from __future__ import annotations

import json
import socket
import tempfile
import threading
import unittest
from pathlib import Path

from gif_player_ipc import build_widget_cmd, daemon_send
from gif_player_paths import get_paths


class IpcTests(unittest.TestCase):
    def test_protocol_v2_command_mapping(self):
        self.assertEqual(
            build_widget_cmd("cat-2", ["move", "12", "34"]),
            {"action": "move", "id": "cat-2", "x": 12.0, "y": 34.0},
        )
        self.assertEqual(
            build_widget_cmd("*", ["jump-rate", "4.5"]),
            {"action": "jump-rate", "id": "*", "seconds": 4.5},
        )

    def test_json_line_roundtrip(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            paths = get_paths(env={"XDG_RUNTIME_DIR": temp}, home=root, allow_legacy=False)
            paths.ensure_runtime_dir()
            ready = threading.Event()
            received = {}

            def server():
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
                    listener.bind(str(paths.socket_path))
                    listener.listen(1)
                    ready.set()
                    connection, _ = listener.accept()
                    with connection:
                        line = connection.recv(4096)
                        received.update(json.loads(line.decode().strip()))
                        connection.sendall(b'{"ok": true, "protocol": 2}\n')

            thread = threading.Thread(target=server, daemon=True)
            thread.start()
            self.assertTrue(ready.wait(2))
            response = daemon_send(paths, {"action": "ping"})
            thread.join(2)
            self.assertEqual(received, {"action": "ping"})
            self.assertEqual(response, {"ok": True, "protocol": 2})


if __name__ == "__main__":
    unittest.main()
