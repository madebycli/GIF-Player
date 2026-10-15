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
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--noctalia", required=True, help="directory containing the noctalia executable")
parser.add_argument("--binary", default="target/release/gif-player")
parser.add_argument("--examples", default="target/release/examples")
parser.add_argument("--artifacts", default="artifacts-noctalia")
parser.add_argument("--scheme", default="Dracula", help="built-in colour scheme to switch to at run time")
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
        (artifacts / "noctalia-host-result.json").write_text(json.dumps({
            "ok": True,
            "noctalia": run(["noctalia", "--version"]).stdout.strip(),
            "scheme_before": before,
            "scheme_after": appearance()["palette"],
            "checks": ["install-plugin attaches the bar widget", "native picker opens through noctalia msg",
                       "real pointer clicks on native tiles start three GIFs", "native control panel opens",
                       "all fourteen colour roles reach the daemon", "Rust application window opens",
                       "run-time colour-scheme change reaches the daemon"],
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
