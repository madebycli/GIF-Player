#!/usr/bin/env python3
"""Installed entry point for the GTK3 live control panel."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys

from gif_player_bootstrap import (
    configure_control,
    load_legacy,
    picker_command,
    require_wayland,
    validate_graphics,
)
from gif_player_paths import get_paths


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="gif-control", description="GIF Player Control")
    parser.add_argument("--gif-dir")
    args = parser.parse_args(argv)
    paths = get_paths(args.gif_dir)
    try:
        require_wayland()
        paths.ensure_runtime_dir()
        module = load_legacy("gif-control.py", "gif_player_legacy_control")
        validate_graphics(module)
        configure_control(module, paths)

        def open_picker(_self, *_args):
            env = os.environ.copy()
            env["GIF_PLAYER_GIF_DIR"] = str(paths.gif_dir)
            subprocess.Popen(
                picker_command(),
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )

        module.ControlWindow._open_picker = open_picker
        module.main()
        return 0
    except RuntimeError as exc:
        print(f"gif-control: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
