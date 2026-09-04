"""Read-only tests: all network mutations are mocked."""

import ctypes
import io
import json
import subprocess
import unittest
from unittest.mock import patch

import network_helper as network


def profile(**overrides):
    return {
        "name": "Renamed connection",
        "uuid": "saved-uuid",
        "ssid": "Café:東京\\Wi-Fi",
        "device": "",
        "hidden": False,
        "bssid": "",
        "key": "wpa-psk",
        **overrides,
    }


def access_point(**overrides):
    return {
        "ssid": "Café:東京\\Wi-Fi",
        "bssid": "AA:BB:CC:DD:EE:FF",
        "device": "wlan0",
        "protected": True,
        "advanced": False,
        "hidden": False,
        "requiresSsid": False,
        "uuid": "",
        **overrides,
    }


def decode_password_file(payload):
    key, value = payload.removesuffix("\n").split(":", 1)
    if key != "802-11-wireless-security.psk":
        raise AssertionError("Unexpected password-file key")
    # The NM parser strips unescaped boundary whitespace, then uses the same
    # C/octal escape semantics as GLib g_strcompress for NUL-free secrets.
    value = value.strip(" \t\r\n\v\f").encode("ascii")
    glib = ctypes.CDLL("libglib-2.0.so.0")
    glib.g_strcompress.argtypes = [ctypes.c_char_p]
    glib.g_strcompress.restype = ctypes.c_void_p
    glib.g_free.argtypes = [ctypes.c_void_p]
    decoded = glib.g_strcompress(value)
    try:
        return ctypes.string_at(decoded).decode("utf-8")
    finally:
        glib.g_free(decoded)


class NetworkTests(unittest.TestCase):
    def run_main(self, text, command="action"):
        output, errors = io.StringIO(), io.StringIO()
        with patch("sys.argv", ["network_helper.py", command]), patch("sys.stdin", io.StringIO(text)), patch("sys.stdout", output), patch("sys.stderr", errors):
            network.main()
        self.assertEqual(errors.getvalue(), "")
        return json.loads(output.getvalue())

    def test_escaped_unicode_records(self):
        self.assertEqual(
            network.records("*:Café\\:東京\\\\Wi-Fi:AA\\:BB:90:WPA2:wlan0\n", 6),
            [["*", "Café:東京\\Wi-Fi", "AA:BB", "90", "WPA2", "wlan0"]],
        )
        self.assertEqual(network.records(":a\\\\\\:b::\n", 4), [["", "a\\:b", "", ""]])

    def test_invalid_record_is_not_offline(self):
        for text in ("unterminated\\", "ssid\nwith\nnewlines"):
            with self.assertRaises(network.NetworkError):
                network.records(text, 6)

    @patch("network_helper.query")
    def test_empty_optional_profile_fields_preserve_literal_ssid(self, query):
        query.side_effect = [
            [["Renamed open profile", "saved-uuid", "802-11-wireless"]],
            network.records("connection.interface-name:\n802-11-wireless.ssid:--\n802-11-wireless.hidden:no\n802-11-wireless.bssid:\n", 2),
        ]
        saved = network.wifi_profiles()[0]
        self.assertEqual((saved["device"], saved["bssid"], saved["key"]), ("", "", ""))
        self.assertEqual(saved["ssid"], "--")
        self.assertEqual(network.profile_kind(saved), "open")
        entries = network.build_networks(
            [["", "--", "AA:BB", "90", "", "wlan0"]],
            [{"name": "wlan0", "managed": True}],
            [saved],
            {},
        )
        self.assertEqual(len(entries), 1)
        self.assertTrue(entries[0]["saved"])

    def test_invalid_signal_data_has_sanitized_error(self):
        with self.assertRaisesRegex(network.NetworkError, "invalid Wi-Fi signal data"):
            network.build_networks(
                [["", "Home", "AA:BB", "secret123", "WPA2", "wlan0"]],
                [{"name": "wlan0", "managed": True}],
                [],
                {},
            )

    @patch("network_helper.nmcli")
    def test_malformed_requests_never_invoke_nmcli(self, run):
        requests = [
            None, [], "secret123", 1, {}, {"operation": None},
            {"operation": "wifi"}, {"operation": "wifi", "enabled": "false"},
            {"operation": "wifi", "enabled": 0},
            {"operation": "connect"},
            *({"operation": "connect", "network": value} for value in (None, [], "secret123", 1)),
            *({"operation": "connect", "network": access_point(), "password": value} for value in (None, [], {}, 123, "\ud800")),
            {"operation": "connect", "network": access_point(), "ssid": None},
            *({"operation": "connect", "network": access_point(**{field: None}), "password": "secret123"} for field in ("device", "bssid", "uuid", "ssid")),
            *({"operation": "connect", "network": access_point(**{field: "false"}), "password": "secret123"} for field in ("advanced", "hidden", "requiresSsid", "protected")),
            {"operation": "connect", "network": access_point(device="bad\0device"), "password": "secret123"},
            {"operation": "connect", "network": access_point(bssid=""), "password": "secret123"},
        ]
        for index, request in enumerate(requests):
            with self.subTest(case=index):
                result = self.run_main(json.dumps(request))
                self.assertFalse(result["ok"])
                self.assertTrue(result["error"])
                self.assertNotIn("secret123", repr(result))
        run.assert_not_called()

    @patch("network_helper.nmcli")
    def test_invalid_json_error_does_not_echo_request(self, run):
        result = self.run_main('{"password":"secret123"')
        self.assertEqual(result, {"ok": False, "error": "The network request is not valid JSON."})
        run.assert_not_called()

    @patch("network_helper.subprocess.run", side_effect=PermissionError("secret123"))
    def test_nmcli_os_error_is_explicit_and_sanitized(self, run):
        result = self.run_main(json.dumps({"operation": "connect", "network": access_point(), "password": "secret123"}))
        self.assertFalse(result["ok"])
        self.assertIn("Check permissions", result["error"])
        self.assertNotIn("secret123", repr(result))

    @patch("network_helper.status", side_effect=RuntimeError("programming fault"))
    def test_unexpected_programming_fault_is_not_swallowed(self, status):
        with self.assertRaisesRegex(RuntimeError, "programming fault"):
            self.run_main("", "status")

    def test_multiple_adapters_and_renamed_saved_profile(self):
        ssid = profile()["ssid"]
        adapters = [{"name": "wlan0", "managed": True}, {"name": "wlan1", "managed": True}]
        aps = [
            ["", ssid, "AA:BB", "95", "WPA2", "wlan0"],
            ["*", ssid, "AA:CC", "50", "WPA2", "wlan0"],
            ["", ssid, "AA:DD", "85", "WPA2", "wlan1"],
        ]
        entries = network.build_networks(aps, adapters, [profile()], {"wlan0": "saved-uuid"})
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries[0]["bssid"], "AA:CC")
        self.assertEqual({entry["device"] for entry in entries}, {"wlan0", "wlan1"})
        self.assertTrue(all(entry["saved"] for entry in entries))
        self.assertEqual(entries[0]["name"], ssid)

    def test_bssid_pinned_saved_profile_is_preserved(self):
        adapters = [{"name": "wlan0", "managed": True}]
        aps = [["", "Home", "AA:BB", "20", "WPA2", "wlan0"], ["", "Home", "AA:CC", "90", "WPA2", "wlan0"]]
        entries = network.build_networks(aps, adapters, [profile(ssid="Home", bssid="AA:BB")], {})
        self.assertEqual(len(entries), 2)
        saved = next(entry for entry in entries if entry["saved"])
        self.assertEqual(saved["bssid"], "AA:BB")
        self.assertEqual(next(entry for entry in entries if not entry["saved"])["bssid"], "AA:CC")

    def test_distinct_bssid_pinned_profiles_remain_selectable(self):
        adapters = [{"name": "wlan0", "managed": True}]
        aps = [["", "Home", "AA:BB", "20", "WPA2", "wlan0"], ["*", "Home", "AA:CC", "90", "WPA2", "wlan0"]]
        profiles = [
            profile(uuid="first", ssid="Home", bssid="AA:BB"),
            profile(uuid="second", ssid="Home", bssid="AA:CC"),
        ]
        entries = network.build_networks(aps, adapters, profiles, {"wlan0": "second"})
        self.assertEqual(len(entries), 2)
        self.assertEqual(len({entry["id"] for entry in entries}), 2)
        self.assertEqual({entry["uuid"]: entry["bssid"] for entry in entries}, {"first": "AA:BB", "second": "AA:CC"})
        self.assertTrue(all(entry["saved"] and entry["available"] for entry in entries))
        self.assertEqual([entry["uuid"] for entry in entries if entry["active"]], ["second"])

    def test_unpinned_duplicate_profiles_remain_selectable(self):
        adapters = [{"name": "wlan0", "managed": True}]
        aps = [["", "Home", "AA:BB", "90", "WPA2", "wlan0"], ["*", "Home", "AA:CC", "20", "WPA2", "wlan0"]]
        profiles = [profile(uuid="first", ssid="Home"), profile(uuid="second", ssid="Home")]
        entries = network.build_networks(aps, adapters, profiles, {"wlan0": "second"})
        self.assertEqual(len(entries), 2)
        self.assertEqual(len({entry["id"] for entry in entries}), 2)
        self.assertEqual({entry["uuid"]: entry["bssid"] for entry in entries}, {"first": "AA:BB", "second": "AA:CC"})
        self.assertTrue(all(entry["saved"] and entry["available"] for entry in entries))
        self.assertEqual([entry["uuid"] for entry in entries if entry["active"]], ["second"])

    def test_hidden_enterprise_and_out_of_range(self):
        adapters = [{"name": "wlan0", "managed": True}]
        aps = [["", "", "AA:BB", "20", "WPA2", "wlan0"], ["", "Office", "AA:CC", "90", "WPA2 802.1X", "wlan0"]]
        entries = network.build_networks(aps, adapters, [profile(hidden=True)], {})
        self.assertTrue(next(entry for entry in entries if entry["name"] == "Office")["advanced"])
        self.assertTrue(next(entry for entry in entries if entry["name"] == "Hidden network")["requiresSsid"])
        saved = next(entry for entry in entries if entry["saved"])
        self.assertFalse(saved["available"])
        self.assertTrue(saved["hidden"])
        self.assertFalse(saved["requiresSsid"])

    def test_saved_activity_is_specific_to_adapter(self):
        adapters = [{"name": "wlan0", "managed": True}, {"name": "wlan1", "managed": True}]
        entries = network.build_networks([], adapters, [profile()], {"wlan0": "saved-uuid"})
        self.assertTrue(next(entry for entry in entries if entry["device"] == "wlan0")["active"])
        self.assertFalse(next(entry for entry in entries if entry["device"] == "wlan1")["active"])

    @patch("network_helper.nmcli")
    def test_new_password_uses_ask_stdin_only(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, "private-value", "private-value")
        result = network.action({"operation": "connect", "network": access_point(), "password": "private-value"})
        args = run.call_args.args[0]
        self.assertIn("--ask", args)
        self.assertNotIn("password", args)
        self.assertNotIn("private-value", repr(args))
        self.assertEqual(run.call_args.kwargs["stdin"], "private-value\n")
        self.assertNotIn("private-value", repr(result))

    @patch("network_helper.nmcli")
    def test_saved_password_uses_password_file_stdin_only(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, "", "")
        passwords = [
            " leading:trailing ",
            "\tleading and trailing\t",
            " \t both \t ",
            "literal\\t\\n\\101\\",
            "\\ trailing\\ ",
            "東京 café\\:\t ",
            " \v\f ",
        ]
        for index, password in enumerate(passwords):
            with self.subTest(case=index):
                network.action({"operation": "connect", "network": access_point(uuid="saved-uuid"), "password": password})
                args = run.call_args.args[0]
                self.assertEqual(args[-2:], ["passwd-file", "/dev/stdin"])
                self.assertEqual(decode_password_file(run.call_args.kwargs["stdin"]), password)
                self.assertNotIn(password, args)

    @patch("network_helper.nmcli")
    def test_saved_and_open_connections_need_no_prompt(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, "", "")
        for entry in (access_point(uuid="saved-uuid"), access_point(protected=False)):
            network.action({"operation": "connect", "network": entry})
            self.assertNotIn("--ask", run.call_args.args[0])
            self.assertIsNone(run.call_args.kwargs["stdin"])

    @patch("network_helper.nmcli")
    def test_hidden_connection_preserves_exact_ssid(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, "", "")
        network.action({"operation": "connect", "network": access_point(hidden=True, requiresSsid=True), "ssid": " :東京\\ ", "password": "secret123"})
        self.assertIn(" :東京\\ ", run.call_args.args[0])
        self.assertEqual(run.call_args.args[0][-2:], ["hidden", "yes"])

    @patch("network_helper.nmcli")
    def test_unsafe_password_or_enterprise_never_runs_command(self, run):
        for request in (
            {"operation": "connect", "network": access_point(), "password": "line\nbreak"},
            {"operation": "connect", "network": access_point(advanced=True), "password": "secret123"},
        ):
            with self.assertRaises(network.NetworkError):
                network.action(request)
        run.assert_not_called()

    @patch("network_helper.nmcli")
    def test_failure_diagnostics_cannot_expose_secrets(self, run):
        run.return_value = subprocess.CompletedProcess([], 4, "secret123", "secret123")
        result = network.action({"operation": "connect", "network": access_point(), "password": "secret123"})
        self.assertFalse(result["ok"])
        self.assertTrue(result["needsPassword"])
        self.assertNotIn("secret123", repr(result))

    @patch("network_helper.nmcli")
    def test_wifi_toggle_does_not_disable_networking(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, "", "")
        network.action({"operation": "wifi", "enabled": False})
        self.assertEqual(run.call_args.args[0], ["--wait", "10", "radio", "wifi", "off"])

    @patch("network_helper.wifi_profiles", return_value=[])
    @patch("network_helper.query")
    def test_cached_status_preserves_ethernet_and_never_scans(self, query, profiles):
        query.side_effect = [
            [["connected", "full", "enabled", "enabled"]],
            [["eth0", "ethernet", "connected", "Cable", "wired"], ["wlan0", "wifi", "disconnected", "", ""], ["docker0", "bridge", "connected (externally)", "docker", "bridge"]],
            [],
        ]
        state = network.status()
        self.assertTrue(state["connected"])
        self.assertEqual(state["deviceName"], "eth0")
        self.assertEqual(len(state["connections"]), 1)
        self.assertTrue(state["wifiAvailable"])
        self.assertEqual(query.call_args.args[0][-2:], ["--rescan", "no"])


if __name__ == "__main__":
    unittest.main()
