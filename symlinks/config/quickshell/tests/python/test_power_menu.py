"""Check action vectors without loading Quickshell or executing session actions."""

import json
from pathlib import Path
import re
import unittest


MENU = Path(__file__).resolve().parents[2] / "PowerMenu.qml"


class PowerMenuTests(unittest.TestCase):
    def test_session_and_system_action_vectors(self):
        source = MENU.read_text(encoding="utf-8")
        actions = {
            name: json.loads(command)
            for name, command in re.findall(
                r'\b(\w+):\s*\{\s*label:\s*"[^"]+",\s*command:\s*(\[[^\]]*\])\s*\}',
                source,
            )
        }
        self.assertEqual(
            actions,
            {
                "logout": ["uwsm", "stop"],
                "reboot": ["systemctl", "reboot"],
                "shutdown": ["systemctl", "poweroff"],
            },
        )


if __name__ == "__main__":
    unittest.main()
