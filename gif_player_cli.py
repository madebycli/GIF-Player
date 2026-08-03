#!/usr/bin/env python3
"""Shell-independent CLI for the GIF Player supervisor."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from gif_player_bootstrap import LIBEXEC_DIR, configure_main, load_legacy, require_wayland
from gif_player_ipc import build_widget_cmd, daemon_send, ensure_daemon
from gif_player_paths import AppPaths, get_paths

KNOWN_COMMANDS = {
    "run", "ipc", "all", "list", "edit", "lock", "stop-all", "kill-all",
    "picker", "control", "daemon", "self-test", "doctor",
}


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
        description="GTK3-Wayland-GIF-Overlay mit Supervisor-Daemon und IPC v2",
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

    sub.add_parser("list", help="Laufende Widget-IDs anzeigen")
    sub.add_parser("edit", help="Alle Widgets entsperren")
    sub.add_parser("lock", help="Alle Widgets sperren")
    sub.add_parser("stop-all", aliases=["kill-all"], help="Alle Widgets beenden")
    sub.add_parser("picker", help="Picker öffnen")
    sub.add_parser("control", help="Control-Panel öffnen")
    sub.add_parser("daemon", help="Supervisor-Daemon (intern/manuell)")
    sub.add_parser("self-test", help="XDG-Pfade und Runtime-Sicherheit prüfen")
    sub.add_parser("doctor", help="Python-Abhängigkeiten und GTK-Typelibs prüfen")
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
