#!/usr/bin/env python3
"""Test real Wayland surfaces and input with Sway; not an application runtime dependency."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument("--binary", default="target/release/gif-player")
parser.add_argument("--examples", default="target/release/examples")
parser.add_argument("--artifacts", default="artifacts")
args = parser.parse_args()
if not shutil.which("grim"):
    parser.error("grim is required for real surface/screenshot assertions")
binary = str(Path(args.binary).resolve())
examples = Path(args.examples).resolve()
artifacts = Path(args.artifacts).resolve()
artifacts.mkdir(parents=True, exist_ok=True)
children = []


def wait_for(check, description):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.03)
    raise AssertionError("Timed out: " + description)


with tempfile.TemporaryDirectory(prefix="gif-wayland-") as directory:
    root = Path(directory)
    runtime = root / "runtime"
    runtime.mkdir(mode=0o700)
    env = os.environ | {
        "XDG_RUNTIME_DIR": str(runtime),
        "XDG_CONFIG_HOME": str(root / "config"),
        "XDG_DATA_HOME": str(root / "data"),
        "XDG_CACHE_HOME": str(root / "cache"),
        "WLR_BACKENDS": "headless",
        "WLR_HEADLESS_OUTPUTS": "2",
        "WLR_RENDERER": "pixman",
    }
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("DISPLAY", None)
    config = root / "sway.conf"
    config.write_text("xwayland disable\noutput HEADLESS-1 mode 1280x720 position 0 0\noutput HEADLESS-2 mode 1920x1080 position 1280 0 scale 1.5\nseat seat0 fallback true\n")

    def launch(argv, name, **kwargs):
        logfile = open(artifacts / (name + ".log"), "w")
        child = subprocess.Popen(argv, env=env, stdout=logfile, stderr=logfile, **kwargs)
        children.append(child)
        return child

    def request(value):
        with socket.socket(socket.AF_UNIX) as stream:
            stream.settimeout(4)
            stream.connect(str(runtime / "gif-player/daemon.sock"))
            stream.sendall((json.dumps(value) + "\n").encode())
            return json.loads(stream.makefile().readline())

    try:
        launch(["sway", "-c", str(config)], "sway")
        wait_for(lambda: any(p.is_socket() and p.name.startswith("wayland-") for p in runtime.iterdir()), "Wayland socket")
        env["WAYLAND_DISPLAY"] = next(p.name for p in runtime.iterdir() if p.is_socket() and p.name.startswith("wayland-"))
        env["SWAYSOCK"] = str(next(runtime.glob("sway-ipc.*.sock")))
        fixture = root / "fixture.gif"
        subprocess.run([str(examples / "generate_fixture"), str(fixture)], check=True)
        launch([binary, "daemon", "--stay", "--no-restore"], "player")
        wait_for(lambda: (runtime / "gif-player/daemon.sock").exists(), "player socket")
        launch([str(examples / "input_probe")], "input-probe")
        probe_log = artifacts / "input-probe.log"
        wait_for(lambda: "FULLSCREEN" in probe_log.read_text(), "fullscreen XDG window")
        subprocess.run(["swaymsg", '[app_id="gif-player-input-probe"] move to output HEADLESS-1', '[app_id="gif-player-input-probe"] fullscreen enable'], env=env, check=True, stdout=subprocess.DEVNULL)
        driver = subprocess.Popen([str(examples / "virtual_input")], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        children.append(driver)
        assert driver.stdout.readline().strip() == "ready"
        time.sleep(0.2)

        def input_command(command):
            driver.stdin.write(command + "\n")
            driver.stdin.flush()
            assert driver.stdout.readline().strip() == "done " + command
            time.sleep(0.08)

        def clicks():
            return probe_log.read_text().count('"event":"press"')

        assert request({"action": "spawn", "gif": str(fixture), "id": "test", "state": {"x": 100, "y": 100, "scale": 1, "output": "HEADLESS-1"}})["ok"]
        wait_for(lambda: not request({"action": "status", "id": "test"})["loading"], "GIF decode")
        input_command("move 115 115")
        before = clicks()
        input_command("click")
        assert clicks() == before + 1, "locked GIF intercepts a click"
        input_command("scroll 15")
        input_command("key 30")
        assert '"event":"scroll"' in probe_log.read_text()
        assert '"utf8":"a"' in probe_log.read_text()
        if shutil.which("grim"):
            picture = artifacts / "fullscreen-overlay.png"
            subprocess.run(["grim", "-o", "HEADLESS-1", str(picture)], env=env, check=True)
            pixel = json.loads(subprocess.check_output([str(examples / "png_pixel"), str(picture), "115", "115"]))
            assert pixel != [36, 96, 160, 255], "GIF is hidden under fullscreen"
        request({"action": "canvas", "id": "test", "enabled": True})
        before = clicks()
        input_command("move 800 500")
        input_command("click")
        assert clicks() == before + 1, "locked canvas intercepts transparent-area input"
        request({"action": "unlock", "id": "test"})
        before = clicks()
        input_command("click")
        assert clicks() == before + 1, "edit canvas captures the whole screen"
        input_command("move 115 115")
        before = clicks()
        input_command("down")
        assert clicks() == before, "edit region does not capture the GIF"
        input_command("move 215 215")
        input_command("up")
        state = request({"action": "status", "id": "test"})
        assert state["base_position"] == [200, 200], state
        request({"action": "lock", "id": "test"})
        request({"action": "move", "id": "test", "x": -500, "y": 3000})
        assert request({"action": "status", "id": "test"})["base_position"] == [-500, 3000]
        request({"action": "output", "id": "test", "output": "HEADLESS-2"})
        request({"action": "corner", "id": "test", "position": "center"})
        state = request({"action": "snapshot"})
        assert len(state["outputs"]) == 2
        assert state["widgets"][0]["output"] == "HEADLESS-2"
        # The genuine Rust control surface must work at fractional scale too.
        assert request({"action": "control"})["ok"]
        time.sleep(0.3)
        ui_output = state["outputs"][0]["name"]
        if shutil.which("grim"):
            subprocess.run(["grim", "-o", ui_output, str(artifacts / "standalone.png")], env=env, check=True)
        def ui_ready():
            picture = artifacts / "standalone.png"
            subprocess.run(["grim", "-o", ui_output, str(picture)], env=env, check=True)
            pixel = json.loads(subprocess.check_output([str(examples / "png_pixel"), str(picture), "800", "70"]))
            return 0 < pixel[0] < 40
        wait_for(ui_ready, "standalone surface first presentation")
        input_command("move " + str((1280 if ui_output == "HEADLESS-2" else 0) + 220) + " 140")
        input_command("click")
        (artifacts / "standalone-state.json").write_text(json.dumps(request({"action":"snapshot"}),indent=2))
        wait_for(lambda: request({"action": "status", "id": "test"})["paused"], "standalone UI Pause all")
        request({"action": "ui-close"})
        subprocess.run(["swaymsg", "output HEADLESS-2 disable"], env=env, check=True, stdout=subprocess.DEVNULL)
        wait_for(lambda: len(request({"action": "snapshot"})["outputs"]) == 1, "output removal")
        request({"action": "quit-daemon"})
        (artifacts / "result.json").write_text(json.dumps({"ok": True, "compositor": "Sway", "checks": ["fullscreen overlay", "locked GIF click-through", "locked transparent canvas", "keyboard and scroll pass-through", "edit-region isolation", "drag", "offscreen", "two outputs", "fractional scale", "standalone UI interaction", "output removal"]}, indent=2))
        print("11 real Wayland acceptance checks passed")
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
        for child in reversed(children):
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
