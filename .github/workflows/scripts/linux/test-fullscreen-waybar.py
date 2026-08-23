#!/usr/bin/env python3

import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import MagicMock, patch


# exec_module() on a source path writes __pycache__ next to the script, which
# would land inside the symlinks/ tree that this test loads from.
sys.dont_write_bytecode = True

ROOT = Path(os.environ["DIR"])
SCRIPT = ROOT / "symlinks/config/hypr/scripts/fullscreen-waybar.py"
SPEC = importlib.util.spec_from_file_location("fullscreen_waybar", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
fullscreen_waybar = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fullscreen_waybar)


class FullscreenWaybarTests(unittest.TestCase):
    def test_reads_active_workspace_fullscreen_state(self) -> None:
        with patch.object(
            fullscreen_waybar.subprocess,
            "check_output",
            return_value='{"hasfullscreen": true}',
        ):
            self.assertTrue(fullscreen_waybar.active_workspace_fullscreen())

    def test_invalid_workspace_state_is_ignored(self) -> None:
        with patch.object(
            fullscreen_waybar.subprocess,
            "check_output",
            side_effect=subprocess.CalledProcessError(1, "hyprctl"),
        ):
            self.assertIsNone(fullscreen_waybar.active_workspace_fullscreen())

    def test_waybar_is_toggled_when_fullscreen_state_changes(self) -> None:
        state = {"hidden": False}
        with patch.object(fullscreen_waybar, "toggle_waybar", return_value=True) as toggle:
            fullscreen_waybar.set_hidden(True, state)

        toggle.assert_called_once_with()
        self.assertTrue(state["hidden"])

    def test_waybar_is_not_toggled_when_fullscreen_state_is_unchanged(self) -> None:
        state = {"hidden": True}
        with patch.object(fullscreen_waybar, "toggle_waybar") as toggle:
            fullscreen_waybar.set_hidden(True, state)

        toggle.assert_not_called()

    def test_failed_toggle_does_not_change_tracked_state(self) -> None:
        state = {"hidden": False}
        with patch.object(fullscreen_waybar, "toggle_waybar", return_value=False):
            fullscreen_waybar.set_hidden(True, state)

        self.assertFalse(state["hidden"])

    def test_closed_ipc_restores_waybar_and_requests_restart(self) -> None:
        connection = MagicMock()
        connection.recv.return_value = b""
        socket = MagicMock()
        socket.return_value.__enter__.return_value = connection

        with (
            patch.dict(
                os.environ,
                {
                    "HYPRLAND_INSTANCE_SIGNATURE": "test",
                    "XDG_RUNTIME_DIR": "/run/user/test",
                },
            ),
            patch.object(fullscreen_waybar.socket, "socket", socket),
            patch.object(fullscreen_waybar, "update"),
            patch.object(fullscreen_waybar, "set_hidden") as set_hidden,
            patch.object(sys, "stderr", new_callable=io.StringIO) as stderr,
        ):
            self.assertEqual(fullscreen_waybar.main(), 1)

        set_hidden.assert_called_once_with(False, {"hidden": False})
        self.assertIn("IPC stream closed", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
