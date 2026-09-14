#!/usr/bin/env python3
"""Shell-independent CLI for the GIF Player supervisor."""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import subprocess
import sys
import time
from pathlib import Path

from gif_player_bootstrap import LIBEXEC_DIR, configure_main, load_legacy, require_wayland
from gif_player_ipc import build_widget_cmd, daemon_send, ensure_daemon
from gif_player_paths import AppPaths, get_paths

KNOWN_COMMANDS = {
    "run", "ipc", "all", "list", "watch", "catalog", "profiles",
    "edit", "lock", "stop-all", "kill-all", "picker", "control", "daemon",
    "self-test", "doctor",
}

PROFILE_KEYS = (
    "x", "y", "scale", "opacity", "flip_h", "flip_v", "speed",
    "bouncing", "jumping", "jump_rate",
)


def resolve_gif(value: str, gif_dir: Path) -> Path:
    candidate = Path(value).expanduser()
    if candidate.is_file():
        return candidate.resolve()
    direct = gif_dir / candidate
    if direct.is_file():
        return direct.resolve()

    wanted = candidate.name.lower()
    if wanted.endswith(".gif"):
        wanted = wanted[:-4]
    matches = sorted(
        path.resolve()
        for path in gif_dir.rglob("*")
        if path.is_file() and path.suffix.lower() == ".gif" and path.stem.lower() == wanted
    ) if gif_dir.is_dir() else []
    if not matches:
        raise FileNotFoundError(f"GIF '{value}' nicht gefunden in {gif_dir}")
    if len(matches) > 1:
        choices = ", ".join(str(path.relative_to(gif_dir)) for path in matches[:8])
        raise RuntimeError(f"GIF-Name '{value}' ist mehrdeutig: {choices}")
    return matches[0]


def _catalog(gif_dir: Path) -> list[dict[str, object]]:
    if not gif_dir.is_dir():
        return []
    result: list[dict[str, object]] = []
    for path in sorted(gif_dir.rglob("*"), key=lambda item: str(item).lower()):
        if not path.is_file() or path.suffix.lower() != ".gif":
            continue
        resolved = path.resolve()
        relative = path.relative_to(gif_dir)
        try:
            info = path.stat()
            size = info.st_size
            mtime_ns = info.st_mtime_ns
        except OSError:
            size = 0
            mtime_ns = 0
        result.append({
            "name": path.stem,
            "relative": str(relative),
            "category": relative.parts[0] if len(relative.parts) > 1 else "",
            "path": str(resolved),
            "size": size,
            "mtime_ns": mtime_ns,
        })
    return result


class ProfileStore:
    """Small compatibility store for the existing profiles.json format."""

    def __init__(self, path: Path):
        self.path = path
        self.lock_path = path.with_name(".profiles.lock")

    def load(self) -> dict[str, dict]:
        try:
            if not self.path.exists():
                return {}
            data = json.loads(self.path.read_text(encoding="utf-8"))
            return data if isinstance(data, dict) else {}
        except (OSError, json.JSONDecodeError):
            return {}

    def write(self, profiles: dict[str, dict]) -> None:
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with self.lock_path.open("w", encoding="utf-8") as lock_file:
            fcntl.flock(lock_file, fcntl.LOCK_EX)
            tmp = self.path.with_name(self.path.name + ".tmp")
            tmp.write_text(
                json.dumps(profiles, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            os.replace(tmp, self.path)


def _snapshot_widgets(paths: AppPaths) -> tuple[list[dict], str | None]:
    response = daemon_send(paths, {"action": "list"})
    if not response.get("ok"):
        return [], str(response.get("error") or "Daemon läuft nicht")
    widgets = []
    for status in response.get("widgets", []):
        if not isinstance(status, dict) or not status.get("file"):
            continue
        entry = {"gif": status["file"]}
        for key in PROFILE_KEYS:
            if key in status:
                entry[key] = status[key]
        widgets.append(entry)
    return widgets, None


def _profile_command(paths: AppPaths, args: argparse.Namespace) -> int:
    store = ProfileStore(paths.profile_file)
    profiles = store.load()
    action = args.profile_action

    if action == "list":
        return _print_result({"ok": True, "profiles": profiles})

    if action in {"save", "overwrite"}:
        widgets, error = _snapshot_widgets(paths)
        if error:
            return _print_result({"error": error})
        if not widgets:
            return _print_result({"error": "Keine Widgets aktiv"})
        if action == "save" and args.name in profiles:
            return _print_result({"error": f"Profil '{args.name}' existiert bereits"})
        profiles[args.name] = {"widgets": widgets}
        store.write(profiles)
        return _print_result({"ok": True, "name": args.name, "widgets": len(widgets)})

    if action == "apply":
        profile = profiles.get(args.name)
        if not isinstance(profile, dict):
            return _print_result({"error": f"Profil '{args.name}' nicht gefunden"})
        widgets = profile.get("widgets", [])
        if not isinstance(widgets, list):
            return _print_result({"error": f"Profil '{args.name}' ist ungültig"})
        return _print_result(daemon_send(
            paths,
            {"action": "apply-setup", "widgets": widgets},
            timeout=15.0,
        ))

    if action == "rename":
        if args.old not in profiles:
            return _print_result({"error": f"Profil '{args.old}' nicht gefunden"})
        if args.new in profiles:
            return _print_result({"error": f"Profil '{args.new}' existiert bereits"})
        profiles[args.new] = profiles.pop(args.old)
        store.write(profiles)
        return _print_result({"ok": True, "old": args.old, "name": args.new})

    if action == "delete":
        if args.name not in profiles:
            return _print_result({"error": f"Profil '{args.name}' nicht gefunden"})
        profiles.pop(args.name, None)
        store.write(profiles)
        return _print_result({"ok": True, "deleted": args.name})

    return _print_result({"error": f"Unbekannte Profilaktion: {action}"})


def _watch(paths: AppPaths, interval: float) -> int:
    """Long-lived JSONL bridge for Noctalia.

    This is a compatibility implementation for the migration branch. It keeps a
    single process alive and only emits snapshots when state changes or on a
    low-frequency heartbeat. The final native daemon will expose event-driven
    subscription without polling, while preserving this CLI surface.
    """

    interval = max(0.25, min(float(interval), 10.0))
    last_payload = ""
    last_emit = 0.0
    heartbeat = 5.0
    try:
        while True:
            response = daemon_send(paths, {"action": "list"}, timeout=1.0)
            if response.get("ok"):
                payload = {
                    "ok": True,
                    "type": "snapshot",
                    "widgets": response.get("widgets", []),
                }
            else:
                payload = {
                    "ok": False,
                    "type": "offline",
                    "widgets": [],
                    "error": response.get("error", "Daemon läuft nicht"),
                }
            encoded = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
            now = time.monotonic()
            if encoded != last_payload or now - last_emit >= heartbeat:
                print(encoded, flush=True)
                last_payload = encoded
                last_emit = now
            time.sleep(interval)
    except KeyboardInterrupt:
        return 0


def _extract_gif_dir(argv: list[str]) -> tuple[str | None, list[str]]:
    result: list[str] = []
    gif_dir: str | None = None
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg == "--gif-dir":
            if index + 1 >= len(argv):
                raise SystemExit("--gif-dir benötigt einen Pfad")
            gif_dir = argv[index + 1]
            index += 2
            continue
        if arg.startswith("--gif-dir="):
            gif_dir = arg.split("=", 1)[1]
            index += 1
            continue
        result.append(arg)
        index += 1
    return gif_dir, result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="gif-player",
        description="Wayland-GIF-Overlay mit Supervisor-Daemon und IPC v2",
        epilog="Ohne Unterbefehl wird der Picker geöffnet. Ein GIF kann direkt per Name gestartet werden.",
    )
    parser.add_argument(
        "--gif-dir",
        metavar="DIR",
        help="GIF-Verzeichnis (vor GIF_PLAYER_GIF_DIR und XDG-Standardpfad)",
    )
    sub = parser.add_subparsers(dest="command")

    run = sub.add_parser("run", help="GIF per Pfad oder Name starten")
    run.add_argument("gif")
    run.add_argument("--id")
    run.add_argument("--monitor", type=int)
    run.add_argument("--state", help="JSON mit Startwerten")

    ipc = sub.add_parser("ipc", help="Befehl an eine Widget-ID senden")
    ipc.add_argument("widget_id")
    ipc.add_argument("action_args", nargs="+")

    all_parser = sub.add_parser("all", help="Befehl an alle Widgets senden")
    all_parser.add_argument("action_args", nargs="+")

    list_parser = sub.add_parser("list", help="Laufende Widget-IDs anzeigen")
    list_parser.add_argument("--json", action="store_true", help="vollständigen Daemon-Status als JSON ausgeben")

    watch = sub.add_parser("watch", help="Statusänderungen als JSONL-Stream ausgeben")
    watch.add_argument("--interval", type=float, default=1.0, help="Kompatibilitäts-Pollintervall in Sekunden")

    sub.add_parser("catalog", help="GIF-Sammlung als JSON ausgeben")

    profiles = sub.add_parser("profiles", help="Setups/Profile verwalten")
    profile_sub = profiles.add_subparsers(dest="profile_action", required=True)
    profile_sub.add_parser("list", help="Profile als JSON ausgeben")
    for action in ("save", "overwrite", "apply", "delete"):
        p = profile_sub.add_parser(action)
        p.add_argument("name")
    rename = profile_sub.add_parser("rename")
    rename.add_argument("old")
    rename.add_argument("new")

    sub.add_parser("edit", help="Alle Widgets entsperren")
    sub.add_parser("lock", help="Alle Widgets sperren")
    sub.add_parser("stop-all", aliases=["kill-all"], help="Alle Widgets beenden")
    sub.add_parser("picker", help="Picker öffnen")
    sub.add_parser("control", help="Control-Panel öffnen")
    sub.add_parser("daemon", help="Supervisor-Daemon (intern/manuell)")
    sub.add_parser("self-test", help="XDG-Pfade und Runtime-Sicherheit prüfen")
    sub.add_parser("doctor", help="Runtime-Abhängigkeiten prüfen")
    return parser


def _launch(entry: str, gif_dir: str | None = None) -> int:
    env = os.environ.copy()
    if gif_dir:
        env["GIF_PLAYER_GIF_DIR"] = str(Path(gif_dir).expanduser().absolute())
    try:
        subprocess.Popen(
            [sys.executable, str(LIBEXEC_DIR / entry)],
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except Exception as exc:
        print(f"Start fehlgeschlagen: {exc}", file=sys.stderr)
        return 1
    return 0


def _print_result(result: dict) -> int:
    print(json.dumps(result, ensure_ascii=False))
    return 1 if "error" in result else 0


def _run_daemon(paths: AppPaths) -> int:
    try:
        require_wayland()
        paths.ensure_runtime_dir()
        module = load_legacy("gif-script.py", "gif_player_legacy_main")
        configure_main(module, paths)
        return int(module.run_daemon())
    except RuntimeError as exc:
        print(f"gif-player: {exc}", file=sys.stderr)
        return 2


def _doctor(paths: AppPaths) -> int:
    try:
        import cairo  # noqa: F401
        import gi
        from PIL import Image  # noqa: F401

        for namespace, version in (
            ("Gtk", "3.0"),
            ("Gdk", "3.0"),
            ("GdkPixbuf", "2.0"),
            ("GtkLayerShell", "0.1"),
        ):
            gi.require_version(namespace, version)
        from gi.repository import Gdk, GdkPixbuf, Gtk, GtkLayerShell  # noqa: F401,E402
    except Exception as exc:
        print(f"gif-player doctor: {exc}", file=sys.stderr)
        return 1
    print("Python imports and GTK typelibs: OK")
    return 0


def _self_test(paths: AppPaths) -> int:
    paths.ensure_runtime_dir()
    mode = paths.runtime_dir.stat().st_mode & 0o777
    payload = {
        "ok": mode == 0o700,
        "runtime_dir": str(paths.runtime_dir),
        "runtime_mode": oct(mode),
        "config_dir": str(paths.config_dir),
        "cache_dir": str(paths.cache_dir),
        "data_dir": str(paths.data_dir),
        "gif_dir": str(paths.gif_dir),
        "socket": str(paths.socket_path),
        "protocol": 2,
    }
    print(json.dumps(payload, ensure_ascii=False))
    return 0 if payload["ok"] else 1


def main(argv: list[str] | None = None) -> int:
    raw = list(sys.argv[1:] if argv is None else argv)
    try:
        extracted_dir, normalized = _extract_gif_dir(raw)
    except SystemExit as exc:
        print(exc, file=sys.stderr)
        return 2

    if not normalized:
        normalized = ["picker"]
    elif normalized[0] not in KNOWN_COMMANDS and not normalized[0].startswith("-"):
        name, *rest = normalized
        normalized = ["run", name] if not rest else [
            "ipc", name, *(["quit"] if rest[0] == "stop" else rest)
        ]

    parser = _parser()
    args = parser.parse_args((["--gif-dir", extracted_dir] if extracted_dir else []) + normalized)
    paths = get_paths(args.gif_dir)

    if args.command == "self-test":
        return _self_test(paths)
    if args.command == "doctor":
        return _doctor(paths)
    if args.command == "catalog":
        print(json.dumps({"ok": True, "gif_dir": str(paths.gif_dir), "gifs": _catalog(paths.gif_dir)}, ensure_ascii=False))
        return 0
    if args.command == "profiles":
        return _profile_command(paths, args)
    if args.command == "watch":
        return _watch(paths, args.interval)
    if args.command == "daemon":
        return _run_daemon(paths)
    if args.command == "picker":
        try:
            require_wayland()
        except RuntimeError as exc:
            print(f"gif-player: {exc}", file=sys.stderr)
            return 2
        return _launch("gif_picker_entry.py", args.gif_dir)
    if args.command == "control":
        try:
            require_wayland()
        except RuntimeError as exc:
            print(f"gif-player: {exc}", file=sys.stderr)
            return 2
        return _launch("gif_control_entry.py", args.gif_dir)

    if args.command == "run":
        try:
            require_wayland()
            gif = resolve_gif(args.gif, paths.gif_dir)
        except (RuntimeError, FileNotFoundError) as exc:
            print(f"gif-player: {exc}", file=sys.stderr)
            return 1
        if not ensure_daemon(paths, LIBEXEC_DIR / "gif_player_cli.py"):
            print("gif-player: Daemon nicht erreichbar", file=sys.stderr)
            return 1
        command: dict = {"action": "spawn", "gif": str(gif)}
        if args.id:
            command["id"] = args.id
        if args.monitor is not None:
            command["monitor"] = args.monitor
        if args.state:
            try:
                command["state"] = json.loads(args.state)
            except json.JSONDecodeError as exc:
                print(f"gif-player: ungültiges --state JSON: {exc}", file=sys.stderr)
                return 2
        return _print_result(daemon_send(paths, command, timeout=5.0))

    if args.command == "ipc":
        command = build_widget_cmd(args.widget_id, args.action_args)
        return _print_result(command if "error" in command else daemon_send(paths, command))

    if args.command == "all":
        command = build_widget_cmd("*", args.action_args)
        return _print_result(command if "error" in command else daemon_send(paths, command))

    if args.command == "list":
        response = daemon_send(paths, {"action": "list"})
        if args.json:
            return _print_result(response)
        if not response.get("ok"):
            return 0
        for status in response.get("widgets", []):
            print(status.get("id", "?"))
        return 0

    if args.command in {"edit", "lock"}:
        action = "unlock" if args.command == "edit" else "lock"
        response = daemon_send(paths, {"action": action, "id": "*"})
        if not response.get("ok"):
            print("Keine Widgets aktiv")
            return 0
        results = response.get("results", {})
        if not results:
            print("Keine Widgets aktiv")
        for widget_id, result in sorted(results.items()):
            print(f"{widget_id}: {result.get('error', action)}")
        return 0

    if args.command in {"stop-all", "kill-all"}:
        response = daemon_send(paths, {"action": "stop-all"})
        count = response.get("stopped", 0) if response.get("ok") else 0
        print(f"{count} Widget(s) beendet" if count else "Keine Widgets aktiv")
        return 0

    parser.print_help()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
