#!/usr/bin/env python3
"""Installed entry point for the GTK3 GIF picker."""

from __future__ import annotations

import argparse
import sys

from gif_player_bootstrap import (
    configure_picker,
    load_legacy,
    require_wayland,
    validate_graphics,
)
from gif_player_paths import get_paths


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="gif-picker", description="GIF Player Picker")
    parser.add_argument("--gif-dir")
    args = parser.parse_args(argv)
    paths = get_paths(args.gif_dir)
    try:
        require_wayland()
        paths.ensure_runtime_dir()
        paths.ensure_config_dir()
        paths.ensure_cache_dir()
        paths.ensure_gif_dir()
        module = load_legacy("gif-picker.py", "gif_player_legacy_picker")
        validate_graphics(module)
        configure_picker(module, paths)
        module.main()
        return 0
    except RuntimeError as exc:
        print(f"gif-picker: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
