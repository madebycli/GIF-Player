from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SourceContractTests(unittest.TestCase):
    def test_protocol_v2_actions_remain_present(self):
        source = (ROOT / "gif-script.py").read_text(encoding="utf-8")
        for action in (
            "ping", "list", "spawn", "stop-all", "apply-setup", "quit-daemon",
            "status", "lock", "unlock", "toggle", "pause", "play", "move",
            "move-by", "scale", "corner", "opacity", "flip", "speed",
            "bounce", "stop-bounce", "hop", "jump", "jump-rate", "reset", "quit",
        ):
            self.assertIn(f'"{action}"', source)

    def test_multiple_instance_allocation_is_preserved(self):
        source = (ROOT / "gif-script.py").read_text(encoding="utf-8")
        self.assertIn('while f"{base}-{n}" in self.widgets', source)
        self.assertIn('wid = self.allocate_id(Path(gif).stem)', source)

    def test_bootstrap_overrides_all_legacy_runtime_paths(self):
        source = (ROOT / "gif_player_bootstrap.py").read_text(encoding="utf-8")
        for assignment in (
            "module.RUNTIME_DIR = paths.runtime_dir",
            "module.DAEMON_SOCK = paths.socket_path",
            "module.DAEMON_LOCK = paths.daemon_lock",
            "module.STATE_FILE = paths.state_file",
            "module.LOG_FILE = paths.daemon_log",
            "module.GIF_DIR = paths.gif_dir",
            "module.CACHE_DIR = paths.thumbnail_cache",
            "module.PROFILE_FILE = paths.profile_file",
        ):
            self.assertIn(assignment, source)

    def test_runtime_patches_are_activated_and_packaged(self):
        bootstrap = (ROOT / "gif_player_bootstrap.py").read_text(encoding="utf-8")
        package = (ROOT / "nix" / "package.nix").read_text(encoding="utf-8")
        patch = (ROOT / "gif_player_runtime_patch.py").read_text(encoding="utf-8")
        guard = (ROOT / "gif_player_runtime_guard.py").read_text(encoding="utf-8")
        self.assertIn(
            "from gif_player_runtime_patch import install_runtime_patches",
            bootstrap,
        )
        self.assertIn(
            "from gif_player_runtime_guard import install_transition_guards",
            bootstrap,
        )
        self.assertIn("install_runtime_patches(module)", bootstrap)
        self.assertIn("install_transition_guards(module)", bootstrap)
        for filename in (
            "gif_player_runtime.py",
            "gif_player_runtime_patch.py",
            "gif_player_runtime_guard.py",
        ):
            self.assertIn(filename, package)
        for marker in (
            '"surface-to-canvas-begin"',
            '"surface-to-canvas-end"',
            "manual_position(x, y)",
            "bounce_step(",
            "advance_frame_timeline(",
        ):
            self.assertIn(marker, patch)
        self.assertIn('self._surface_phase == "to-canvas"', guard)
        self.assertIn("canvas_ready", guard)
        self.assertIn("self._apply_surface_mode()", guard)

    def test_canonical_launchers_have_no_global_python_path(self):
        sources = "\n".join(
            (ROOT / filename).read_text(encoding="utf-8")
            for filename in (
                "gif_player_cli.py",
                "gif_player_ipc.py",
                "gif_picker_entry.py",
                "gif_control_entry.py",
            )
        )
        self.assertNotIn("/usr/bin/python3", sources)
        self.assertNotIn('subprocess.Popen(["python3"', sources)
        self.assertNotIn("~/Scripts/Gif-Overlay", sources)


if __name__ == "__main__":
    unittest.main()
