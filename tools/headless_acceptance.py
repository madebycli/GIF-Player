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
        "XDG_STATE_HOME": str(root / "state"),
        "GIF_PLAYER_LOG": "trace",
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
        player = launch([binary, "daemon", "--stay", "--no-restore"], "player")
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
            pixel = json.loads(subprocess.check_output([str(examples / "png_pixel"), str(picture), "350", "600"]))
            return pixel == [18,18,23,255]
        wait_for(ui_ready, "standalone surface first presentation")
        # The floating panel is centred (900x688 on a 1280x720 logical output): its Pause button is the
        # first control of the primary action row at panel position (322, 172).
        input_command("move " + str((1280 if ui_output == "HEADLESS-2" else 0) + 190 + 322) + " " + str(16 + 172))
        input_command("click")
        (artifacts / "standalone-state.json").write_text(json.dumps(request({"action":"snapshot"}),indent=2))
        wait_for(lambda: request({"action": "status", "id": "test"})["paused"], "standalone primary playback action")
        request({"action": "ui-close"})
        assert request({"action":"app"})["ok"]
        def app_is_tiled():
            tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree", "-r"], env=env))
            def walk(node, floating=False):
                if node.get("app_id") == "io.github.madebycli.NoctaliaGifPlayer": return not floating
                return any(walk(child, floating) for child in node.get("nodes",[])) or any(walk(child,True) for child in node.get("floating_nodes",[]))
            return walk(tree)
        wait_for(app_is_tiled, "normal tiled xdg application window")
        subprocess.run(["swaymsg", '[app_id="gif-player-input-probe"] fullscreen disable'],env=env,check=True,stdout=subprocess.DEVNULL)
        subprocess.run(["swaymsg", '[app_id="io.github.madebycli.NoctaliaGifPlayer"] focus'],env=env,check=True,stdout=subprocess.DEVNULL)
        time.sleep(0.1)
        # Real keyboard behaviour of the app window: typing, Enter, key repeat and the clipboard.
        shutil.copy(fixture, root / "zebra.gif")
        assert request({"action": "library-directory", "path": str(root)})["ok"]
        time.sleep(0.4)
        def widget_ids():
            return {widget["id"] for widget in request({"action": "snapshot"})["widgets"]}
        input_command("ctrl-f")
        input_command("text ZEBRA")
        input_command("key 28")
        wait_for(lambda: "zebra" in widget_ids(), "typed search followed by Enter starts the first match")
        assert request({"action": "quit", "id": "zebra"})["ok"]
        # The compositor only advertises the repeat rate; the client must repeat a held key itself.
        driver.stdin.write("keydown 14\n")
        driver.stdin.flush()
        assert driver.stdout.readline().strip() == "done keydown 14"
        time.sleep(1.6)
        input_command("keyup 14")
        input_command("key 28")
        wait_for(lambda: "test" not in widget_ids(), "held Backspace repeated and emptied the query")
        clipboard_checked = False
        if shutil.which("wl-copy") and shutil.which("wl-paste"):
            subprocess.run(["wl-copy", "zebra"], env=env, check=True, timeout=5)
            input_command("ctrl-f")
            input_command("ctrl-v")
            input_command("key 28")
            wait_for(lambda: "zebra" in widget_ids(), "clipboard text pasted into the search field")
            assert request({"action": "quit", "id": "zebra"})["ok"]
            input_command("ctrl-f")
            input_command("ctrl-c")
            copied = subprocess.run(["wl-paste", "-n"], env=env, capture_output=True, text=True, timeout=5).stdout
            assert copied == "zebra", ("Ctrl+C did not publish the selection", copied)
            clipboard_checked = True
        input_command("ctrl-w")
        def app_is_closed():
            tree=json.loads(subprocess.check_output(["swaymsg","-t","get_tree","-r"],env=env))
            def contains(node):
                return node.get("app_id")=="io.github.madebycli.NoctaliaGifPlayer" or any(contains(child) for child in node.get("nodes",[])+node.get("floating_nodes",[]))
            return not contains(tree)
        wait_for(app_is_closed,"real app keyboard shortcut")
        # Closing the window while the pointer rested on a tile must leave the daemon idle (a hover
        # request once ping-ponged with cache release and spun at full speed after the close).
        def cpu_seconds():
            fields = Path(f"/proc/{player.pid}/stat").read_text().rsplit(")", 1)[1].split()
            return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")
        time.sleep(0.5)
        busy_before = cpu_seconds()
        time.sleep(1.5)
        assert cpu_seconds() - busy_before < 0.25, "daemon spins after the application window closed"
        subprocess.run(["swaymsg", '[app_id="gif-player-input-probe"] focus'],env=env,check=True,stdout=subprocess.DEVNULL)
        subprocess.run(["swaymsg", '[app_id="gif-player-input-probe"] fullscreen enable'],env=env,check=True,stdout=subprocess.DEVNULL)
        native = root / "native.gif"
        subprocess.run([str(examples / "generate_fixture"), str(native), "1024", "24"], check=True)
        assert request({"action":"spawn","gif":str(native),"id":"quality","state":{"x":400,"y":100,"scale":1,"output":"HEADLESS-1","paused":True,"locked":False}})["ok"]
        wait_for(lambda: not request({"action":"status","id":"quality"})["loading"], "native resolution decode")
        status = request({"action":"status","id":"quality"})
        assert status["source_size"] == [1024,1024], status
        assert status["frame_storage"] == "lossless-disk", status
        input_command("move 420 120")
        input_command("down")
        presentation_ms = []
        for dx in [20,25,30,35,40,50]:
            start = time.monotonic()
            driver.stdin.write("move " + str(420 + dx) + " 120\n")
            driver.stdin.flush()
            assert driver.stdout.readline().strip().startswith("done ")
            picture = artifacts / "native-drag.png"
            def drag_is_presented():
                # The virtual input roundtrip acknowledges submission, not presentation.
                subprocess.run(["grim", "-o", "HEADLESS-1", str(picture)], env=env, check=True)
                pixel = json.loads(subprocess.check_output([str(examples / "png_pixel"), str(picture), str(400 + dx + 509), "120"]))
                return pixel == [22,24,40,255] and request({"action":"status","id":"quality"})["base_position"] == [400 + dx,100]
            wait_for(drag_is_presented, "drag position and rendered leading pixel")
            trailing = json.loads(subprocess.check_output([str(examples / "png_pixel"), str(picture), "405", "140"]))
            assert trailing == [36,96,160,255], ("stale previous buffer left a trail", trailing)
            position = request({"action":"status","id":"quality"})["base_position"]
            assert position == [400 + dx,100], position
            presentation_ms.append((time.monotonic() - start) * 1000)
        input_command("up")
        (artifacts / "native-drag.json").write_text(json.dumps({"native_size":status["source_size"],"storage":status["frame_storage"],"presentation_ms":presentation_ms,"max_ms":max(presentation_ms)},indent=2))
        subprocess.run(["swaymsg", "output HEADLESS-2 disable"], env=env, check=True, stdout=subprocess.DEVNULL)
        wait_for(lambda: len(request({"action": "snapshot"})["outputs"]) == 1, "output removal")
        report = request({"action": "logs-export", "directory": str(artifacts / "diagnostics")})
        assert report.get("ok"), report
        wait_for(lambda: request({"action":"logs-export-status","directory":report["directory"]}).get("pending") != True, "asynchronous trace export")
        records = [json.loads(line) for line in (artifacts / "diagnostics/daemon.jsonl").read_text().splitlines()]
        for event in ["connected", "outputs_changed", "frame_decoded", "frame_committed", "frame_callback", "drag_motion", "input_region"]:
            assert any(record.get("event") == event for record in records), f"missing real diagnostic event: {event}"
        assert (artifacts / "diagnostics/snapshot.json").exists()
        request({"action": "quit-daemon"})
        checks = ["fullscreen overlay", "locked GIF click-through", "locked transparent canvas", "keyboard and scroll pass-through", "edit-region isolation", "drag", "offscreen", "two outputs", "fractional scale", "standalone UI interaction", "normal tiled application window", "normal app keyboard focus and shortcut", "idle daemon after closing the window", "typed search and Enter", "client-side key repeat", ] + (["clipboard paste and copy"] if clipboard_checked else []) + ["native resolution under cache pressure", "unsnapped visual drag", "output removal", "asynchronous diagnostic trace export"]
        (artifacts / "result.json").write_text(json.dumps({"ok": True, "compositor": "Sway", "checks": checks}, indent=2))
        print(f"{len(checks)} real Wayland acceptance checks passed")
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
