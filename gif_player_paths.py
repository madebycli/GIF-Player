#!/usr/bin/env python3
"""XDG path handling for GIF Player.

This module intentionally depends only on the Python standard library so path
checks and CLI help can run without importing GTK or requiring a display.
"""

from __future__ import annotations

import os
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping

APP_NAME = "gif-player"
LEGACY_GIF_DIR = Path.home() / "Scripts" / "Gif-Overlay" / "Gifs"


def _absolute_env_path(env: Mapping[str, str], name: str, fallback: Path) -> Path:
    value = env.get(name)
    if value:
        candidate = Path(value).expanduser()
        if candidate.is_absolute():
            return candidate
    return fallback


@dataclass(frozen=True)
class AppPaths:
    runtime_dir: Path
    config_dir: Path
    cache_dir: Path
    data_dir: Path
    gif_dir: Path

    @property
    def socket_path(self) -> Path:
        return self.runtime_dir / "daemon.sock"

    @property
    def daemon_lock(self) -> Path:
        return self.runtime_dir / "daemon.lock"

    @property
    def daemon_log(self) -> Path:
        return self.runtime_dir / "daemon.log"

    @property
    def state_file(self) -> Path:
        return self.config_dir / "state.json"

    @property
    def profile_file(self) -> Path:
        return self.config_dir / "profiles.json"

    @property
    def thumbnail_cache(self) -> Path:
        return self.cache_dir / "thumbs"

    def ensure_runtime_dir(self) -> Path:
        """Create a private runtime directory and reject unsafe ownership/types."""
        self.runtime_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        info = self.runtime_dir.lstat()
        if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
            raise RuntimeError(f"Unsicheres Runtime-Verzeichnis: {self.runtime_dir}")
        if info.st_uid != os.getuid():
            raise RuntimeError(
                f"Runtime-Verzeichnis gehört UID {info.st_uid}, erwartet {os.getuid()}: "
                f"{self.runtime_dir}"
            )
        os.chmod(self.runtime_dir, 0o700)
        return self.runtime_dir

    def ensure_config_dir(self) -> Path:
        self.config_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        return self.config_dir

    def ensure_cache_dir(self) -> Path:
        self.cache_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        return self.cache_dir

    def ensure_gif_dir(self) -> Path:
        if self.gif_dir.exists():
            if not self.gif_dir.is_dir():
                raise RuntimeError(f"GIF-Pfad ist kein Verzeichnis: {self.gif_dir}")
            return self.gif_dir
        try:
            self.gif_dir.relative_to("/nix/store")
        except ValueError:
            pass
        else:
            raise RuntimeError(
                f"GIF-Verzeichnis im schreibgeschützten Nix Store kann nicht erstellt werden: "
                f"{self.gif_dir}"
            )
        self.gif_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        return self.gif_dir


def get_paths(
    cli_gif_dir: str | os.PathLike[str] | None = None,
    *,
    env: Mapping[str, str] | None = None,
    home: Path | None = None,
    allow_legacy: bool = True,
) -> AppPaths:
    env = os.environ if env is None else env
    home = Path.home() if home is None else Path(home)

    runtime_base = env.get("XDG_RUNTIME_DIR")
    if runtime_base and Path(runtime_base).is_absolute():
        runtime_dir = Path(runtime_base) / APP_NAME
    else:
        runtime_dir = Path("/tmp") / f"{APP_NAME}-{os.getuid()}"

    config_home = _absolute_env_path(env, "XDG_CONFIG_HOME", home / ".config")
    cache_home = _absolute_env_path(env, "XDG_CACHE_HOME", home / ".cache")
    data_home = _absolute_env_path(env, "XDG_DATA_HOME", home / ".local" / "share")

    config_dir = config_home / APP_NAME
    cache_dir = cache_home / APP_NAME
    data_dir = data_home / APP_NAME

    if cli_gif_dir is not None:
        gif_dir = Path(cli_gif_dir).expanduser().absolute()
    elif env.get("GIF_PLAYER_GIF_DIR"):
        gif_dir = Path(env["GIF_PLAYER_GIF_DIR"]).expanduser().absolute()
    else:
        xdg_gif_dir = data_dir / "gifs"
        legacy = home / "Scripts" / "Gif-Overlay" / "Gifs"
        gif_dir = legacy if allow_legacy and legacy.is_dir() and not xdg_gif_dir.exists() else xdg_gif_dir

    return AppPaths(
        runtime_dir=runtime_dir,
        config_dir=config_dir,
        cache_dir=cache_dir,
        data_dir=data_dir,
        gif_dir=gif_dir,
    )
