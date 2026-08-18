#!/usr/bin/env python3

import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch


# exec_module() on a source path writes __pycache__ next to the script, which
# would land inside the symlinks/ tree that this test loads from.
sys.dont_write_bytecode = True

ROOT = Path(os.environ["DIR"])
SCRIPT = ROOT / "symlinks/config/hypr/scripts/workspaces.py"
SPEC = importlib.util.spec_from_file_location("workspaces", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
workspaces = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(workspaces)


def emit(workspace: int) -> dict:
    out = io.StringIO()
    with patch.object(sys, "stdout", out):
        workspaces.emit(workspace)
    return json.loads(out.getvalue())


class WorkspacesTests(unittest.TestCase):
    def test_active_workspace_renders_the_active_icon(self) -> None:
        with patch.object(
            workspaces, "read_state", return_value={"active": 2, "occupied": [1, 2]}
        ):
            payload = emit(2)

        self.assertEqual(payload["text"], workspaces.ACTIVE_ICON)
        self.assertEqual(payload["class"], "active")

    def test_occupied_workspace_renders_the_occupied_icon(self) -> None:
        with patch.object(
            workspaces, "read_state", return_value={"active": 2, "occupied": [1, 2]}
        ):
            payload = emit(1)

        self.assertEqual(payload["text"], workspaces.OCCUPIED_ICON)
        self.assertEqual(payload["class"], "occupied")

    def test_unused_workspace_is_hidden(self) -> None:
        with patch.object(
            workspaces, "read_state", return_value={"active": 2, "occupied": [1, 2]}
        ):
            payload = emit(3)

        self.assertEqual(payload["text"], "")
        self.assertEqual(payload["class"], "empty")

    def test_missing_cached_state_hides_every_dot(self) -> None:
        with patch.object(workspaces, "read_state", return_value=None):
            self.assertEqual(emit(1)["text"], "")

    def test_query_state_ignores_malformed_workspace_ids(self) -> None:
        with patch.object(
            workspaces.subprocess,
            "check_output",
            side_effect=[
                '{"id": 2}',
                '[{"id": 1}, {"id": "two"}, "three"]',
            ],
        ):
            self.assertEqual(
                workspaces.query_state(), {"active": 2, "occupied": [1]}
            )

    def test_refresh_caches_one_snapshot(self) -> None:
        state = {"active": 2, "occupied": [1, 2]}
        with (
            patch.object(workspaces, "query_state", return_value=state),
            patch.object(workspaces, "write_state") as write_state,
        ):
            self.assertTrue(workspaces.refresh_state())

        write_state.assert_called_once_with(state)

    def test_waybar_is_signalled_on_workspace_changes(self) -> None:
        with (
            patch.object(workspaces, "waybar_pids", return_value=[10, 20]),
            patch.object(workspaces.os, "kill") as kill,
        ):
            workspaces.notify_waybar()

        self.assertEqual(
            [call.args for call in kill.call_args_list],
            [(10, workspaces.WAYBAR_SIGNAL), (20, workspaces.WAYBAR_SIGNAL)],
        )

    def test_exited_waybar_does_not_raise(self) -> None:
        with (
            patch.object(workspaces, "waybar_pids", return_value=[10]),
            patch.object(workspaces.os, "kill", side_effect=ProcessLookupError),
        ):
            workspaces.notify_waybar()

    def test_invalid_arguments_are_rejected(self) -> None:
        with patch.object(sys, "stderr", io.StringIO()):
            for argv in ([], ["0"], ["10"], ["one"], ["1", "2"]):
                self.assertEqual(workspaces.main(argv), 2, argv)


class WaybarConfigTests(unittest.TestCase):
    """The dots are only clickable while every workspace has its own module."""

    def setUp(self) -> None:
        with open(ROOT / "symlinks/config/waybar/config") as handle:
            self.config = json.load(handle)

    def test_every_workspace_has_a_module(self) -> None:
        expected = [
            f"custom/ws{n}" for n in range(1, workspaces.LAST_WORKSPACE + 1)
        ]
        self.assertEqual(self.config["group/workspaces"]["modules"], expected)

    def test_modules_dispatch_their_own_workspace(self) -> None:
        for n in range(1, workspaces.LAST_WORKSPACE + 1):
            module = self.config[f"custom/ws{n}"]
            self.assertEqual(
                module["exec"], f"~/.config/hypr/scripts/workspaces.py {n}"
            )
            # Waybar 0.15 still speaks the legacy Hyprland dispatch protocol,
            # which Hyprland 0.56 rejects, so the Lua form is dispatched here.
            self.assertEqual(
                module["on-click"],
                f"hyprctl dispatch 'hl.dsp.focus({{ workspace = {n} }})'",
            )
            self.assertEqual(module["signal"], workspaces.WAYBAR_SIGNAL_ID)
            self.assertEqual(module["return-type"], "json")

    def test_status_modules_are_separate(self) -> None:
        self.assertEqual(
            self.config["modules-right"],
            [
                "custom/playing",
                "custom/stocks",
                "tray",
                "pulseaudio",
                "battery",
                "clock",
            ],
        )

    def test_volume_is_unpadded_and_scrollable(self) -> None:
        volume = self.config["pulseaudio"]
        self.assertEqual(volume["format"], "\uf028 {volume}%")
        self.assertEqual(volume["format-muted"], "\uf028 0%")
        self.assertIn("on-scroll-up", volume)
        self.assertIn("on-scroll-down", volume)
        self.assertIn("on-click-middle", volume)

    def test_media_and_stocks_are_interactive(self) -> None:
        playing = self.config["custom/playing"]
        self.assertEqual(playing["on-click"], "playerctl --player=spotify play-pause")
        self.assertIn("on-click-middle", playing)
        self.assertIn("on-click-right", playing)
        self.assertEqual(playing["return-type"], "json")

        stocks = self.config["custom/stocks"]
        self.assertEqual(stocks["signal"], 2)
        self.assertIn("--toggle", stocks["on-click"])


if __name__ == "__main__":
    unittest.main()
