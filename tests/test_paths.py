from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from gif_player_cli import _extract_gif_dir, resolve_gif
from gif_player_ipc import daemon_argv
from gif_player_paths import get_paths


class PathTests(unittest.TestCase):
    def test_xdg_layout_and_private_runtime(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env = {
                "XDG_RUNTIME_DIR": str(root / "run"),
                "XDG_CONFIG_HOME": str(root / "config"),
                "XDG_CACHE_HOME": str(root / "cache"),
                "XDG_DATA_HOME": str(root / "data"),
            }
            paths = get_paths(env=env, home=root, allow_legacy=False)
            self.assertEqual(paths.runtime_dir, root / "run" / "gif-player")
            self.assertEqual(paths.config_dir, root / "config" / "gif-player")
            self.assertEqual(paths.cache_dir, root / "cache" / "gif-player")
            self.assertEqual(paths.gif_dir, root / "data" / "gif-player" / "gifs")
            paths.ensure_runtime_dir()
            self.assertEqual(paths.runtime_dir.stat().st_mode & 0o777, 0o700)

    def test_gif_dir_priority(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env = {"GIF_PLAYER_GIF_DIR": str(root / "env")}
            self.assertEqual(get_paths(root / "cli", env=env, home=root).gif_dir, root / "cli")
            self.assertEqual(get_paths(env=env, home=root).gif_dir, root / "env")

    def test_name_resolution_in_subdirectory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            nested = root / "anime" / "mascot.gif"
            nested.parent.mkdir()
            nested.write_bytes(b"GIF89a")
            self.assertEqual(resolve_gif("mascot", root), nested.resolve())

    def test_global_gif_dir_can_appear_after_command(self):
        value, argv = _extract_gif_dir(["run", "mascot", "--gif-dir", "/tmp/gifs"])
        self.assertEqual(value, "/tmp/gifs")
        self.assertEqual(argv, ["run", "mascot"])

    def test_store_creation_is_rejected(self):
        paths = get_paths("/nix/store/does-not-exist/gifs", env={}, home=Path("/tmp"))
        with self.assertRaises(RuntimeError):
            paths.ensure_gif_dir()

    def test_daemon_start_prefers_packaged_wrapper(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = root / "libexec" / "gif-player" / "gif_player_cli.py"
            executable = root / "bin" / "gif-player"
            script.parent.mkdir(parents=True)
            executable.parent.mkdir(parents=True)
            script.write_text("# test\n")
            executable.write_text("#!/bin/sh\n")
            executable.chmod(0o755)

            self.assertEqual(
                daemon_argv(script),
                [str(executable), "daemon"],
            )

    def test_daemon_start_falls_back_to_python_for_source_tree(self):
        with tempfile.TemporaryDirectory() as temp:
            script = Path(temp) / "gif_player_cli.py"
            script.write_text("# test\n")
            argv = daemon_argv(script)
            self.assertEqual(argv[-2:], [str(script), "daemon"])


if __name__ == "__main__":
    unittest.main()
