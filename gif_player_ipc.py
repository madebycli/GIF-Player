#!/usr/bin/env python3
"""Protocol-v2 client helpers shared by CLI and GUI launchers."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import time
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from gif_player_paths import AppPaths

DAEMON_BOOT_TIMEOUT = 6.0


def daemon_send(paths: AppPaths, command: dict[str, Any], timeout: float = 2.0) -> dict[str, Any]:
    if not paths.socket_path.exists():
        return {"error": "Daemon läuft nicht"}
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(timeout)
            client.connect(str(paths.socket_path))
            client.sendall((json.dumps(command) + "\n").encode("utf-8"))
            data = bytearray()
            while True:
                chunk = client.recv(65536)
                if not chunk:
                    break
                data.extend(chunk)
                if data.endswith(b"\n"):
                    break
        return json.loads(data.decode("utf-8").strip()) if data else {"ok": True}
    except Exception as exc:
        return {"error": str(exc)}


def daemon_alive(paths: AppPaths) -> bool:
    return bool(daemon_send(paths, {"action": "ping"}, timeout=0.6).get("ok"))


def daemon_argv(command: Sequence[str] | str | Path) -> list[str]:
    """Normalize a daemon command while preserving packaged wrappers.

    Older callers pass the path to ``gif_player_cli.py``. For a Nix layout,
    transparently resolve that path back to ``$out/bin/gif-player`` so the
    child keeps the wrapper-provided GTK/GI environment.
    """
    if isinstance(command, (str, Path)):
        script = Path(command)
        package_executable = script.parent.parent.parent / "bin" / "gif-player"
        if package_executable.is_file() and os.access(package_executable, os.X_OK):
            return [str(package_executable), "daemon"]
        return [sys.executable, str(script), "daemon"]

    argv = [str(part) for part in command]
    if not argv:
        raise ValueError("Leerer Daemon-Startbefehl")
    return argv


def ensure_daemon(
    paths: AppPaths,
    command: Sequence[str] | str | Path,
    timeout: float = DAEMON_BOOT_TIMEOUT,
) -> bool:
    if daemon_alive(paths):
        return True
    paths.ensure_runtime_dir()

    try:
        argv = daemon_argv(command)
        env = os.environ.copy()
        env.setdefault("PYTHONUNBUFFERED", "1")
        with paths.daemon_log.open("a", encoding="utf-8") as log_file:
            log_file.write(
                time.strftime("%Y-%m-%d %H:%M:%S ")
                + "[launcher] "
                + " ".join(argv)
                + "\n"
            )
            log_file.flush()
            process = subprocess.Popen(
                argv,
                env=env,
                stdout=log_file,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
    except Exception as exc:
        print(f"Daemon-Start fehlgeschlagen: {exc}", file=sys.stderr)
        return False

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if daemon_alive(paths):
            return True
        # Do not fail immediately when this child loses the flock race. A
        # concurrent launcher may still be starting the daemon and socket.
        process.poll()
        time.sleep(0.1)
    return daemon_alive(paths)


def build_widget_cmd(widget_id: str, action_args: list[str]) -> dict[str, Any]:
    if not action_args:
        return {"error": "Keine Aktion angegeben"}
    action, *args = action_args
    command: dict[str, Any] = {"action": action, "id": widget_id}
    try:
        if action == "move":
            command["x"], command["y"] = float(args[0]), float(args[1])
        elif action == "move-by":
            command["dx"], command["dy"] = float(args[0]), float(args[1])
        elif action == "scale":
            command["scale"] = float(args[0])
        elif action == "corner":
            command["position"] = args[0]
        elif action == "opacity":
            command["opacity"] = float(args[0])
        elif action == "flip":
            command["mode"] = args[0]
        elif action == "speed":
            command["speed"] = float(args[0])
        elif action == "jump-rate":
            command["seconds"] = float(args[0])
    except (IndexError, ValueError) as exc:
        return {"error": f"Ungültige Argumente für {action}: {exc}"}
    return command
