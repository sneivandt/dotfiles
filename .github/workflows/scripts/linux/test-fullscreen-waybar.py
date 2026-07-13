#!/usr/bin/env python3

import importlib.util
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch


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

    def test_new_waybar_is_hidden_when_fullscreen_is_already_active(self) -> None:
        state = {"hidden": True, "pids": {10}}
        with (
            patch.object(fullscreen_waybar, "waybar_pids", return_value=[10, 20]),
            patch.object(fullscreen_waybar.os, "kill") as kill,
        ):
            fullscreen_waybar.set_hidden(True, state)

        kill.assert_called_once_with(20, fullscreen_waybar.signal.SIGUSR1)

    def test_new_waybar_stays_visible_when_leaving_fullscreen(self) -> None:
        state = {"hidden": True, "pids": {10}}
        with (
            patch.object(fullscreen_waybar, "waybar_pids", return_value=[10, 20]),
            patch.object(fullscreen_waybar.os, "kill") as kill,
        ):
            fullscreen_waybar.set_hidden(False, state)

        kill.assert_called_once_with(10, fullscreen_waybar.signal.SIGUSR1)


if __name__ == "__main__":
    unittest.main()
