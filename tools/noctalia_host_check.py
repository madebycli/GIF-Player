#!/usr/bin/env python3
"""Run a real, unmodified Noctalia 5 host with this plugin under headless Sway and assert real behaviour.

Not part of CI (it needs a Noctalia 5 build and a software EGL stack); developer/acceptance tool only.
It checks, with genuine pointer input and the host's own theme engine:

  * `gif-player install-plugin` enables the plugin and attaches its widget to the host's bar;
  * the native picker panel opens through `noctalia msg` and real clicks on its image tiles start GIFs;
  * the native control panel opens;
  * the daemon receives *all fourteen* colour roles from the host (the Rust UI draws with them);
  * switching the host's colour scheme at run time reaches the daemon and the standalone Rust window.

Usage:
  tools/noctalia_host_check.py --noctalia /path/to/noctalia/bin --artifacts out [--scheme Dracula]

Noctalia needs EGL. On a machine without a GPU, point glvnd at a software Mesa, e.g. on NixOS
    M=$(nix build nixpkgs#mesa --no-link --print-out-paths)
    __EGL_VENDOR_LIBRARY_FILENAMES=$M/share/glvnd/egl_vendor.d/50_mesa.json LIBGL_DRIVERS_PATH=$M/lib/dri ...
Screenshots (which contain Noctalia's own wallpaper) are written to --artifacts and are never bundled.
"""
import argparse
import json
import os
import shutil
import subprocess
import tempfile
import time
import tomllib
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--noctalia", required=True, help="directory containing the noctalia executable")
parser.add_argument("--binary", default="target/release/gif-player")
parser.add_argument("--examples", default="target/release/examples")
parser.add_argument("--artifacts", default="artifacts-noctalia")
parser.add_argument("--scheme", default="Dracula", help="built-in colour scheme to switch to at run time")
parser.add_argument("--sound-binary", help="also install and exercise the native Nix Sound overlay")
parser.add_argument("--pulse-module-dir", help="optional module path for an isolated audio test server")
args = parser.parse_args()
binary = str(Path(args.binary).resolve())
examples = Path(args.examples).resolve()
artifacts = Path(args.artifacts).resolve()
artifacts.mkdir(parents=True, exist_ok=True)
ROLES = {"surface", "surface_variant", "on_surface", "on_surface_variant", "primary", "on_primary", "secondary",
         "on_secondary", "error", "on_error", "outline", "shadow", "hover", "on_hover"}
children = []


def wait_for(check, description, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError("Timed out: " + description)


def write_gifs(folder):
    # Plain generated animations; no third-party artwork.
    folder.mkdir(parents=True, exist_ok=True)
    for index, name in enumerate(["Aurora", "Bouncing Ball", "Cafe Cat", "Dancing Cat", "Fireplace", "Loading Ring"]):
        subprocess.run([str(examples / "generate_fixture"), str(folder / (name + ".gif")), str(96 + index * 8), "10"], check=True)


with tempfile.TemporaryDirectory(prefix="noctalia-host-") as directory:
    root = Path(directory)
    runtime = root / "run"
    runtime.mkdir(mode=0o700)
    gifs = root / "gifs"
    write_gifs(gifs)
    env = os.environ | {
        "HOME": str(root), "XDG_RUNTIME_DIR": str(runtime), "XDG_CONFIG_HOME": str(root / "config"),
        "XDG_DATA_HOME": str(root / "data"), "XDG_CACHE_HOME": str(root / "cache"), "XDG_STATE_HOME": str(root / "state"),
        "GIF_PLAYER_GIF_DIR": str(gifs), "WLR_BACKENDS": "headless", "WLR_HEADLESS_OUTPUTS": "1", "WLR_RENDERER": "pixman",
        "LIBGL_ALWAYS_SOFTWARE": "1", "EGL_PLATFORM": "wayland",
        "PATH": args.noctalia + ":" + str(Path(binary).parent) + ":" + os.environ["PATH"],
    }
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("DISPLAY", None)
    (root / "sway.conf").write_text("xwayland disable\ndefault_border none\noutput HEADLESS-1 mode 1600x900 position 0 0\nseat seat0 fallback true\n")

    def launch(argv, name):
        log = open(artifacts / (name + ".log"), "w")
        child = subprocess.Popen(argv, env=env, stdout=log, stderr=log)
        children.append(child)
        return child

    def run(argv, **kwargs):
        return subprocess.run(argv, env=env, capture_output=True, text=True, **kwargs)

    def request(value):
        return json.loads(run([binary, "request", json.dumps(value)]).stdout)

    def shot(name):
        time.sleep(1.0)
        subprocess.run(["grim", "-o", "HEADLESS-1", str(artifacts / (name + ".png"))], env=env, check=True)

    try:
        launch(["sway", "-c", str(root / "sway.conf")], "sway")
        wait_for(lambda: any(p.is_socket() and p.name.startswith("wayland-") for p in runtime.iterdir()), "Wayland socket")
        env["WAYLAND_DISPLAY"] = next(p.name for p in runtime.iterdir() if p.is_socket() and p.name.startswith("wayland-"))
        env["SWAYSOCK"] = str(next(runtime.glob("sway-ipc.*.sock")))
        install = run([binary, "install-plugin"])
        assert "attached its widget to bar" in install.stdout, install
        if args.sound_binary:
            env["PULSE_SERVER"] = "unix:" + str(runtime / "pulse.sock")
            pulse = ["pulseaudio", "-n", "--daemonize=no", "--use-pid-file=no", "--exit-idle-time=-1",
                     "--load=module-native-protocol-unix socket=" + str(runtime / "pulse.sock") + " auth-anonymous=1",
                     "--load=module-null-sink sink_name=speakers", "--load=module-null-source source_name=microphone"]
            if args.pulse_module_dir:
                pulse += ["--dl-search-path=" + args.pulse_module_dir]
            launch(pulse, "pulse")
            wait_for(lambda: run(["pactl", "info"]).returncode == 0, "isolated audio server")
            installed_sound = run([args.sound_binary, "install-plugin"])
            assert installed_sound.returncode == 0, installed_sound.stderr
        launch([shutil.which("noctalia", path=env["PATH"])], "noctalia")
        wait_for(lambda: run(["noctalia", "msg", "color-scheme-get"]).returncode == 0, "Noctalia IPC", 40)
        launch([binary, "daemon", "--stay", "--no-restore"], "player")
        wait_for(lambda: (runtime / "gif-player/daemon.sock").exists(), "daemon socket")
        driver = subprocess.Popen([str(examples / "virtual_input")], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        children.append(driver)
        assert driver.stdout.readline().strip() == "ready"

        def point(x, y):
            driver.stdin.write(f"move {int(x * 2560 / 1600)} {int(y * 720 / 900)}\nclick\n")
            driver.stdin.flush()
            assert driver.stdout.readline().startswith("done move")
            assert driver.stdout.readline().startswith("done click")
            time.sleep(0.2)

        def panel(name):
            return run(["noctalia", "msg", "panel-toggle", "madebycli/gif-player:" + name])

        assert panel("picker").returncode == 0
        time.sleep(1.5)
        shot("native-picker")
        # Real clicks on the first three native image tiles (geometry of Noctalia 5.2.1 at 1600x900).
        for x, y in [(590, 298), (790, 298), (990, 298)]:
            point(x, y)
        wait_for(lambda: len(request({"action": "snapshot"})["widgets"]) == 3, "three GIFs started by native tile clicks")
        shot("native-picker-active")
        assert panel("picker").returncode == 0
        assert panel("control").returncode == 0
        time.sleep(1.5)
        shot("native-control")
        assert panel("control").returncode == 0
        def appearance():
            return request({"action": "snapshot"})["settings"].get("appearance", {})
        wait_for(lambda: set(appearance().get("palette", {})) >= ROLES, "all fourteen colour roles reach the daemon")
        before = appearance()["palette"]
        assert request({"action": "app"}).get("ok") is not False
        wait_for(lambda: "NoctaliaGifPlayer" in run(["swaymsg", "-t", "get_tree"]).stdout, "normal Rust application window")
        time.sleep(1.5)
        shot("rust-app-host-theme")
        assert run(["noctalia", "msg", "color-scheme-set", "builtin", args.scheme]).returncode == 0
        wait_for(lambda: appearance()["palette"].get("primary") != before.get("primary"), "live colour-scheme change reaches the daemon")
        time.sleep(1.5)
        shot("rust-app-after-scheme-change")
        # Change the effective host settings, retaining the installed plugins and bar layout.
        settings_file = root / "state/noctalia/settings.toml"
        settings = tomllib.loads(settings_file.read_text())
        shell = settings.setdefault("shell", {})
        shell["corner_radius_scale"] = 0.0
        shell["button_borders"] = False
        shell["input_borders"] = False
        shell["settings_window_translucent"] = True
        shell.setdefault("panel", {}).update({"transparency_mode": "glass", "borders": False})
        settings.setdefault("accessibility", {})["ui_scale"] = 1.25

        def write_toml(data, path=()):
            lines = ["[" + ".".join(json.dumps(p) for p in path) + "]"] if path else []
            for key, value in data.items():
                if not isinstance(value, dict):
                    lines.append(json.dumps(key) + " = " + json.dumps(value))
            for key, value in data.items():
                if isinstance(value, dict):
                    lines += write_toml(value, path + (key,))
            return lines

        updated = settings_file.with_suffix(".new")
        updated.write_text("\n".join(write_toml(settings)) + "\n")
        updated.replace(settings_file)
        wait_for(lambda: appearance().get("style", {}).get("corner_radius_scale") == 0
                 and appearance()["style"]["panel_opacity"] < .56
                 and appearance()["style"]["ui_scale"] == 1.25, "effective shape/transparency/UI scale reaches Rust")
        shot("rust-app-glass-square")
        assert panel("control").returncode == 0
        shot("native-control-glass-square")
        assert panel("control").returncode == 0
        assert run(["swaymsg", "output", "HEADLESS-1", "mode", "800x600"]).returncode == 0
        assert panel("picker").returncode == 0
        shot("native-picker-small-scaled")
        assert panel("picker").returncode == 0
        extra_checks = ["effective corner/border/transparency/UI scale reaches Rust live", "native floating panels at 800x600 and 125% UI scale"]
        if args.sound_binary:
            assert run([args.sound_binary, "sound"]).returncode == 0
            wait_for(lambda: (runtime / "nix-sound/audio.sock").exists(), "native Sound starts its Rust backend")
            state = json.loads(run([args.sound_binary, "snapshot"]).stdout)
            assert state["outputs"] and state["inputs"]
            shot("native-sound-small-scaled")
            assert run(["swaymsg", "output", "HEADLESS-1", "mode", "1366x768"]).returncode == 0
            time.sleep(1.2)
            # Growing an output can retain the host's previous configured surface; reopening
            # negotiates its preferred size on the new monitor without rendering past it.
            assert run([args.sound_binary, "sound"]).returncode == 0
            assert run([args.sound_binary, "sound"]).returncode == 0
            shot("native-sound-wide-scaled")
            response = run([args.sound_binary, "request", json.dumps({"action":"volume","kind":"sink","id":state["outputs"][0]["id"],"volume":.32})])
            assert response.returncode == 0, response.stderr
            extra_checks += ["native Sound plugin opens and starts its event backend", "Sound at 800x600 and 1366x768 with 125% scaling and real audio control"]
        (artifacts / "noctalia-host-result.json").write_text(json.dumps({
            "ok": True,
            "noctalia": run(["noctalia", "--version"]).stdout.strip(),
            "scheme_before": before,
            "scheme_after": appearance()["palette"],
            "checks": ["install-plugin attaches the bar widget", "native picker opens through noctalia msg",
                       "real pointer clicks on native tiles start three GIFs", "native control panel opens",
                       "all fourteen colour roles reach the daemon", "Rust application window opens",
                       "run-time colour-scheme change reaches the daemon"] + extra_checks,
        }, indent=2))
        print("Noctalia host checks passed")
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
        for child in reversed(children):
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
