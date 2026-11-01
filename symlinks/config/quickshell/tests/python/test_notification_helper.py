import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch, Mock

SPEC = importlib.util.spec_from_file_location(
    "notification_helper", Path(__file__).resolve().parents[2] / "notification_helper.py"
)
helper = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(helper)


class NotificationHelperTests(unittest.TestCase):
    def result(self, entries):
        return Mock(stdout=json.dumps(entries))

    @patch.object(helper.subprocess, "run")
    def test_snapshot_normalizes_markup_nulls_and_separates_history(self, run):
        run.side_effect = [
            self.result([{"app_name": "Fixture", "summary": "<b>Literal title</b>",
                          "body": '<b>Hello</b><br>world &amp; friends<img src="file:///private">',
                          "urgency": "critical"}]),
            self.result([{"app_name": None, "summary": None, "body": None}]),
        ]
        result = helper.snapshot()
        self.assertTrue(result["ok"])
        self.assertEqual(result["active"][0]["body"], "Hello\nworld & friends")
        self.assertEqual(result["active"][0]["summary"], "<b>Literal title</b>")
        self.assertTrue(result["active"][0]["critical"])
        self.assertEqual(result["history"][0]["app"], "Notification")
        self.assertEqual([call.args[0] for call in run.call_args_list],
                         [["makoctl", "list", "-j"], ["makoctl", "history", "-j"]])
        self.assertTrue(all(call.kwargs["timeout"] == 3 for call in run.call_args_list))

    @patch.object(helper.subprocess, "run")
    def test_empty_lists_are_successful(self, run):
        run.return_value = self.result([])
        self.assertEqual(helper.snapshot(), {"ok": True, "active": [], "history": []})

    @patch.object(helper.subprocess, "run")
    def test_query_failures_are_not_empty_success_or_private_error_output(self, run):
        for failure in (FileNotFoundError(), subprocess.TimeoutExpired("makoctl", 3),
                        subprocess.CalledProcessError(1, "makoctl", stderr="private text")):
            with self.subTest(failure=type(failure).__name__):
                run.side_effect = [self.result([]), failure]
                result = helper.snapshot()
                self.assertFalse(result["ok"])
                self.assertNotIn("history", result)
                self.assertNotIn("private text", result["error"])

    @patch.object(helper.subprocess, "run")
    def test_malformed_or_wrong_shape_json_is_rejected(self, run):
        for text in ("broken", "{}", "[null]", "[3]"):
            run.return_value = Mock(stdout=text)
            self.assertFalse(helper.snapshot()["ok"])


if __name__ == "__main__":
    unittest.main()
