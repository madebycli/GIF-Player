#!/usr/bin/env python3
"""Load the GTK implementation and inject package-safe paths and commands."""

from __future__ import annotations

import importlib.util
import os
import shutil
import sys
import sysconfig
from pathlib import Path
from types import ModuleType

from gif_player_paths import AppPaths
from gif_player_runtime_guard import install_transition_guards
from gif_player_runtime_patch import install_runtime_patches


def _libexec_dir() -> Path:
    override = os.environ.get("GIF_PLAYER_LIBEXEC_DIR")
    if override:
        return Path(override).expanduser().resolve()
    beside_module = Path(__file__).resolve().parent
    if (beside_module / "gif-script.py").is_file():
        return beside_module
    return Path(sysconfig.get_path("data")) / "libexec" / "gif-player"


LIBEXEC_DIR = _libexec_dir()


def require_wayland() -> None:
    if not os.environ.get("WAYLAND_DISPLAY"):
        raise RuntimeError(
            "GIF Player benötigt eine grafische Wayland-Sitzung mit Layer-Shell-"
            "Unterstützung. WAYLAND_DISPLAY ist nicht gesetzt."
        )


def load_legacy(filename: str, module_name: str) -> ModuleType:
    source = LIBEXEC_DIR / filename
    if not source.is_file():
        raise RuntimeError(f"Installierte Komponente fehlt: {source}")
    spec = importlib.util.spec_from_file_location(module_name, source)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Kann installierte Komponente nicht laden: {source}")
    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)
    except (ImportError, ValueError) as exc:
        raise RuntimeError(
            "GTK3/GtkLayerShell konnte nicht geladen werden. Prüfe PyGObject, "
            "GTK3, gtk-layer-shell und die GObject-Introspection-Typelibs: "
            f"{exc}"
        ) from exc
    return module


def validate_graphics(module: ModuleType) -> None:
    initialized = module.Gtk.init_check(None)
    ok = initialized[0] if isinstance(initialized, tuple) else bool(initialized)
    if not ok:
        raise RuntimeError("GTK konnte die aktuelle Wayland-Anzeige nicht initialisieren.")
    supported = getattr(module.GtkLayerShell, "is_supported", None)
    if callable(supported) and not supported():
        raise RuntimeError(
            "Der aktive Wayland-Compositor unterstützt das Layer-Shell-Protokoll nicht."
        )


def packaged_executable(name: str) -> Path | None:
    """Return a package sibling executable before falling back to PATH."""

    package_candidate = LIBEXEC_DIR.parent.parent / "bin" / name
    if package_candidate.is_file() and os.access(package_candidate, os.X_OK):
        return package_candidate
    path_candidate = shutil.which(name)
    return Path(path_candidate) if path_candidate else None


def daemon_command() -> list[str]:
    executable = packaged_executable("gif-player")
    if executable is not None:
        return [str(executable), "daemon"]
    return [sys.executable, str(LIBEXEC_DIR / "gif_player_cli.py"), "daemon"]


def picker_command() -> list[str]:
    executable = packaged_executable("gif-picker")
    if executable is not None:
        return [str(executable)]
    return [sys.executable, str(LIBEXEC_DIR / "gif_picker_entry.py")]


def configure_main(module: ModuleType, paths: AppPaths) -> None:
    module.RUNTIME_DIR = paths.runtime_dir
    module.DAEMON_SOCK = paths.socket_path
    module.DAEMON_LOCK = paths.daemon_lock
    module.CONFIG_DIR = paths.config_dir
    module.STATE_FILE = paths.state_file
    module.LOG_FILE = paths.daemon_log
    module.STATE = module.StateStore(paths.state_file)

    install_runtime_patches(module)
    install_transition_guards(module)

    def ensure_dirs() -> None:
        paths.ensure_runtime_dir()
        paths.ensure_config_dir()

    module.ensure_dirs = ensure_dirs


def configure_picker(module: ModuleType, paths: AppPaths) -> None:
    from gif_player_ipc import daemon_send as shared_daemon_send
    from gif_player_ipc import ensure_daemon as shared_ensure_daemon

    module.RUNTIME_DIR = paths.runtime_dir
    module.DAEMON_SOCK = paths.socket_path
    module.GIF_DIR = paths.gif_dir
    module.CACHE_DIR = paths.thumbnail_cache
    module.PROFILE_FILE = paths.profile_file
    module.DAEMON_SCRIPT = LIBEXEC_DIR / "gif_player_cli.py"
    module.daemon_send = lambda command, timeout=2.0: shared_daemon_send(
        paths, command, timeout
    )
    module.ensure_daemon = lambda timeout=6.0: shared_ensure_daemon(
        paths, daemon_command(), timeout
    )


def configure_control(module: ModuleType, paths: AppPaths) -> None:
    from gif_player_ipc import daemon_send as shared_daemon_send

    module.RUNTIME_DIR = paths.runtime_dir
    module.DAEMON_SOCK = paths.socket_path
    module.PICKER_SCRIPT = LIBEXEC_DIR / "gif_picker_entry.py"
    module.daemon_send = lambda command, timeout=2.0: shared_daemon_send(
        paths, command, timeout
    )
