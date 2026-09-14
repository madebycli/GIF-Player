#!/usr/bin/env python3
"""Protocol-v2 supervisor for the native Wayland GIF renderer.

The supervisor intentionally uses only the Python standard library. Rendering,
layer-shell surfaces and pointer input live in the Rust renderer process.
"""

from __future__ import annotations

import fcntl
import json
import os
import select
import shutil
import signal
import socket
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

from gif_player_paths import AppPaths

EMPTY_EXIT_SECONDS = 2.0
PROFILE_KEYS = (
    "x",
    "y",
    "scale",
    "opacity",
    "flip_h",
    "flip_v",
    "speed",
    "bouncing",
    "jumping",
    "jump_rate",
)


class StateStore:
    def __init__(self, path: Path):
        self.path = path
        self.lock_path = path.with_name(".state.lock")
        self._thread_lock = threading.Lock()

    def load_all(self) -> dict[str, Any]:
        try:
            data = json.loads(self.path.read_text()) if self.path.exists() else {}
            return data if isinstance(data, dict) else {}
        except (OSError, json.JSONDecodeError):
            return {}

    def get(self, key: str) -> dict[str, Any]:
        value = self.load_all().get(key, {})
        return value if isinstance(value, dict) else {}

    def put(self, key: str, data: dict[str, Any]) -> None:
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with self._thread_lock, self.lock_path.open("w", encoding="utf-8") as lock_file:
            fcntl.flock(lock_file, fcntl.LOCK_EX)
            state = self.load_all()
            state[key] = data
            tmp = self.path.with_suffix(self.path.suffix + ".tmp")
            tmp.write_text(json.dumps(state, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
            os.replace(tmp, self.path)


def renderer_executable() -> Path | None:
    override = os.environ.get("GIF_PLAYER_RENDERER")
    if override:
        candidate = Path(override).expanduser()
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate.resolve()

    found = shutil.which("gif-player-renderer")
    if found:
        return Path(found).resolve()

    here = Path(__file__).resolve().parent
    packaged = here.parent.parent / "bin" / "gif-player-renderer"
    if packaged.is_file() and os.access(packaged, os.X_OK):
        return packaged

    for candidate in (
        here / "renderer" / "target" / "release" / "gif-player-renderer",
        here / "renderer" / "target" / "debug" / "gif-player-renderer",
    ):
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
    return None


class RendererProcess:
    def __init__(
        self,
        renderer: Path,
        widget_id: str,
        gif: Path,
        state: dict[str, Any],
        output: str | None,
        monitor: int | None,
        log_file,
    ):
        self.id = widget_id
        self.gif = gif
        self.state_key = gif.stem
        self.is_primary = widget_id == self.state_key
        self._io_lock = threading.Lock()
        payload = {
            "id": widget_id,
            "gif": str(gif),
            "state": state,
        }
        if output:
            payload["output"] = output
        if monitor is not None:
            payload["monitor"] = int(monitor)
        self.process = subprocess.Popen(
            [str(renderer), "--config-json", json.dumps(payload, separators=(",", ":"))],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log_file,
            text=True,
            bufsize=1,
            close_fds=True,
        )

    def alive(self) -> bool:
        return self.process.poll() is None

    def command(self, payload: dict[str, Any], timeout: float = 3.0) -> dict[str, Any]:
        with self._io_lock:
            if not self.alive():
                return {"error": f"renderer exited with code {self.process.returncode}"}
            if self.process.stdin is None or self.process.stdout is None:
                return {"error": "renderer pipes unavailable"}
            try:
                self.process.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
                self.process.stdin.flush()
                ready, _, _ = select.select([self.process.stdout.fileno()], [], [], timeout)
                if not ready:
                    return {"error": "renderer response timeout"}
                line = self.process.stdout.readline()
                if not line:
                    return {"error": "renderer closed response pipe"}
                result = json.loads(line)
                return result if isinstance(result, dict) else {"error": "invalid renderer response"}
            except (BrokenPipeError, OSError, json.JSONDecodeError) as exc:
                return {"error": str(exc)}

    def close(self) -> None:
        if not self.alive():
            return
        self.command({"action": "quit"}, timeout=1.5)
        try:
            self.process.wait(timeout=1.0)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=1.0)
            except subprocess.TimeoutExpired:
                self.process.kill()


class Manager:
    def __init__(self, paths: AppPaths, renderer: Path, log_file):
        self.paths = paths
        self.renderer = renderer
        self.log_file = log_file
        self.state = StateStore(paths.state_file)
        self.widgets: dict[str, RendererProcess] = {}
        self.lock = threading.RLock()
        self.empty_since = time.monotonic()

    def _allocate_id(self, base: str) -> str:
        if base not in self.widgets:
            return base
        number = 2
        while f"{base}-{number}" in self.widgets:
            number += 1
        return f"{base}-{number}"

    def _persist(self, widget: RendererProcess, status: dict[str, Any]) -> None:
        if not widget.is_primary or not status.get("ok"):
            return
        data = {key: status[key] for key in PROFILE_KEYS if key in status}
        self.state.put(widget.state_key, data)

    def _drop_dead(self) -> None:
        dead = [widget_id for widget_id, widget in self.widgets.items() if not widget.alive()]
        for widget_id in dead:
            self.widgets.pop(widget_id, None)
        if not self.widgets and dead:
            self.empty_since = time.monotonic()

    def spawn(
        self,
        gif: Any,
        widget_id: str | None = None,
        state_override: dict[str, Any] | None = None,
        monitor: int | None = None,
        output: str | None = None,
    ) -> dict[str, Any]:
        with self.lock:
            self._drop_dead()
            if not gif:
                return {"error": "spawn: Parameter 'gif' fehlt"}
            path = Path(str(gif)).expanduser().absolute()
            if not path.is_file():
                return {"error": f"GIF not found: {path}"}
            if widget_id:
                if widget_id in self.widgets:
                    return {"error": f"'{widget_id}' läuft bereits"}
            else:
                widget_id = self._allocate_id(path.stem)

            saved = self.state.get(path.stem)
            startup_state = dict(saved)
            if isinstance(state_override, dict):
                startup_state.update(state_override)
            startup_state["locked"] = True
            startup_state["paused"] = False

            try:
                widget = RendererProcess(
                    self.renderer,
                    widget_id,
                    path,
                    startup_state,
                    output,
                    monitor,
                    self.log_file,
                )
            except OSError as exc:
                return {"error": f"spawn failed: {exc}"}
            self.widgets[widget_id] = widget
            self.empty_since = 0.0

            status = widget.command({"action": "status"}, timeout=6.0)
            if not status.get("ok"):
                widget.close()
                self.widgets.pop(widget_id, None)
                self.empty_since = time.monotonic()
                return {"error": f"spawn failed: {status.get('error', 'renderer unavailable')}"}

            if widget_id != path.stem and not state_override:
                suffix = 1
                tail = widget_id.rsplit("-", 1)
                if len(tail) == 2 and tail[1].isdigit():
                    suffix = max(1, int(tail[1]) - 1)
                screen = status.get("screen", [1920, 1080])
                size = status.get("size", [64, 64])
                x = min(float(status.get("x", 100)) + 36.0 * suffix, float(screen[0]) - float(size[0]))
                y = min(float(status.get("y", 100)) + 36.0 * suffix, float(screen[1]) - float(size[1]))
                status = widget.command({"action": "move", "x": x, "y": y})
            self._persist(widget, status)
            return status

    def close_widget(self, widget_id: str) -> dict[str, Any]:
        with self.lock:
            widget = self.widgets.get(widget_id)
            if widget is None:
                return {"error": f"No widget '{widget_id}' running"}
            status = widget.command({"action": "status"})
            self._persist(widget, status)
            widget.close()
            self.widgets.pop(widget_id, None)
            if not self.widgets:
                self.empty_since = time.monotonic()
            return {"ok": True, "shutdown": True}

    def list_status(self) -> dict[str, Any]:
        with self.lock:
            self._drop_dead()
            statuses = []
            for widget_id in sorted(self.widgets):
                widget = self.widgets[widget_id]
                status = widget.command({"action": "status"})
                if status.get("ok"):
                    statuses.append(status)
                    self._persist(widget, status)
            return {"ok": True, "widgets": statuses}

    def widget_action(self, widget: RendererProcess, command: dict[str, Any]) -> dict[str, Any]:
        if command.get("action") == "quit":
            return self.close_widget(widget.id)
        status = widget.command(command)
        self._persist(widget, status)
        return status

    def dispatch(self, command: dict[str, Any]) -> dict[str, Any]:
        with self.lock:
            self._drop_dead()
            action = str(command.get("action", ""))
            if action == "ping":
                return {"ok": True, "daemon": True, "widgets": len(self.widgets), "renderer": "native-wayland"}
            if action == "list":
                return self.list_status()
            if action == "spawn":
                return self.spawn(
                    command.get("gif"),
                    command.get("id"),
                    command.get("state"),
                    command.get("monitor"),
                    command.get("output"),
                )
            if action == "stop-all":
                count = len(self.widgets)
                for widget_id in list(self.widgets):
                    self.close_widget(widget_id)
                return {"ok": True, "stopped": count}
            if action == "apply-setup":
                for widget_id in list(self.widgets):
                    self.close_widget(widget_id)
                results = []
                counts: dict[str, int] = {}
                for entry in command.get("widgets", []):
                    if not isinstance(entry, dict):
                        continue
                    gif = str(entry.get("gif", ""))
                    stem = Path(gif).stem
                    counts[stem] = counts.get(stem, 0) + 1
                    widget_id = stem if counts[stem] == 1 else f"{stem}-{counts[stem]}"
                    state = {key: entry[key] for key in PROFILE_KEYS if key in entry}
                    results.append(
                        self.spawn(
                            gif,
                            widget_id,
                            state,
                            entry.get("monitor"),
                            entry.get("output"),
                        )
                    )
                return {
                    "ok": True,
                    "applied": sum(1 for result in results if result.get("ok")),
                    "results": results,
                }
            if action == "quit-daemon":
                count = len(self.widgets)
                for widget_id in list(self.widgets):
                    self.close_widget(widget_id)
                return {"ok": True, "daemon": False, "stopped": count}

            widget_id = command.get("id")
            if widget_id == "*":
                results = {
                    each: self.widget_action(widget, {**command, "id": each})
                    for each, widget in list(sorted(self.widgets.items()))
                }
                return {"ok": True, "results": results}
            if not widget_id:
                return {"error": f"Aktion '{action}' braucht eine Widget-'id'"}
            widget = self.widgets.get(str(widget_id))
            if widget is None:
                return {"error": f"No widget '{widget_id}' running"}
            return self.widget_action(widget, command)

    def close_all(self) -> None:
        with self.lock:
            for widget_id in list(self.widgets):
                self.close_widget(widget_id)

    def should_exit_empty(self) -> bool:
        with self.lock:
            self._drop_dead()
            return not self.widgets and self.empty_since > 0 and time.monotonic() - self.empty_since >= EMPTY_EXIT_SECONDS


def _read_request(connection: socket.socket) -> dict[str, Any]:
    connection.settimeout(3.0)
    data = bytearray()
    while len(data) < 1024 * 1024:
        chunk = connection.recv(65536)
        if not chunk:
            break
        data.extend(chunk)
        if data.endswith(b"\n"):
            break
    value = json.loads(data.decode("utf-8").strip()) if data else {}
    if not isinstance(value, dict):
        raise ValueError("request must be a JSON object")
    return value


def _serve_connection(connection: socket.socket, manager: Manager, shutdown: threading.Event) -> None:
    try:
        request = _read_request(connection)
        result = manager.dispatch(request)
        if request.get("action") == "quit-daemon" and result.get("ok"):
            shutdown.set()
    except Exception as exc:  # keep protocol errors isolated from the daemon
        result = {"error": f"bad request: {exc}"}
    try:
        connection.sendall((json.dumps(result, ensure_ascii=False) + "\n").encode("utf-8"))
    except OSError:
        pass
    finally:
        try:
            connection.close()
        except OSError:
            pass


def run_daemon(paths: AppPaths) -> int:
    renderer = renderer_executable()
    if renderer is None:
        print("gif-player: gif-player-renderer wurde nicht gefunden", file=os.sys.stderr)
        return 1
    paths.ensure_runtime_dir()
    paths.ensure_config_dir()

    lock_file = paths.daemon_lock.open("w", encoding="utf-8")
    try:
        fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        return 0
    lock_file.write(str(os.getpid()))
    lock_file.flush()

    try:
        paths.socket_path.unlink(missing_ok=True)
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(paths.socket_path))
        os.chmod(paths.socket_path, 0o600)
        server.listen(16)
        server.settimeout(0.25)
    except OSError as exc:
        print(f"gif-player: Socket-Start fehlgeschlagen: {exc}", file=os.sys.stderr)
        return 1

    log_file = paths.daemon_log.open("a", encoding="utf-8", buffering=1)
    manager = Manager(paths, renderer, log_file)
    shutdown = threading.Event()

    def stop(*_args) -> None:
        shutdown.set()

    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, stop)

    workers: list[threading.Thread] = []
    try:
        while not shutdown.is_set():
            if manager.should_exit_empty():
                break
            try:
                connection, _ = server.accept()
            except socket.timeout:
                continue
            except OSError:
                if shutdown.is_set():
                    break
                raise
            worker = threading.Thread(
                target=_serve_connection,
                args=(connection, manager, shutdown),
                daemon=True,
            )
            worker.start()
            workers.append(worker)
            workers = [thread for thread in workers if thread.is_alive()]
    finally:
        manager.close_all()
        try:
            server.close()
        except OSError:
            pass
        paths.socket_path.unlink(missing_ok=True)
        log_file.close()
    return 0
