"""Read-only tests: all network mutations are mocked."""

import ctypes
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

QUICKSHELL_DIR = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(QUICKSHELL_DIR))

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
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix=".network-helper-test-", dir=Path.cwd())
        self.addCleanup(directory.cleanup)
        self.cache_path = Path(directory.name) / "profiles.json"
        cache = patch.object(network, "profile_cache_path", return_value=self.cache_path)
        cache.start()
        self.addCleanup(cache.stop)
        process = patch.object(
            network.subprocess, "run",
            side_effect=AssertionError("Tests must explicitly mock every nmcli process."),
        )
        process.start()
        self.addCleanup(process.stop)

    def run_main(self, text, command="action"):
        output, errors = io.StringIO(), io.StringIO()
        with patch("sys.argv", ["network_helper.py", command]), patch("sys.stdin", io.StringIO(text)), patch("sys.stdout", output), patch("sys.stderr", errors):
            network.main()
        self.assertEqual(errors.getvalue(), "")
        return json.loads(output.getvalue())

    def test_profile_cache_avoids_repeated_detail_queries_and_expires(self):
        inventory = [["Fixture", "uuid", "802-11-wireless"]]
        details = [["802-11-wireless.ssid", "Fixture"]]
        refreshed_details = [["802-11-wireless.ssid", "Changed SSID"]]
        with patch.object(network, "query", side_effect=[inventory, details, inventory, inventory, refreshed_details]) as query, \
             patch.object(network.time, "time", side_effect=[100, 159.999, 160]):
            first = network.wifi_profiles()
            self.assertEqual(first, [profile(name="Fixture", uuid="uuid", ssid="Fixture", key="")])
            self.assertEqual(network.wifi_profiles(), first)
            self.assertEqual(query.call_count, 3)
            refreshed = [{**first[0], "ssid": "Changed SSID"}]
            self.assertEqual(network.wifi_profiles(), refreshed)
            self.assertEqual(query.call_count, 5)
        self.assertEqual(json.loads(self.cache_path.read_text())["profiles"], refreshed)

    def test_profile_inventory_changes_invalidate_cached_profiles(self):
        inventory = [["Fixture", "uuid", "802-11-wireless"]]
        details = [["802-11-wireless.ssid", "Fixture"]]
        with patch.object(network, "query", side_effect=[inventory, details, [], inventory, details]):
            network.wifi_profiles()
            self.assertEqual(network.wifi_profiles(), [])
            network.wifi_profiles()

    def test_disappearing_profile_does_not_fail_status_or_cache_incomplete_inventory(self):
        with patch.object(network, "query", side_effect=[[["Fixture", "uuid", "802-11-wireless"]], []]):
            self.assertEqual(network.wifi_profiles(), [])
        self.assertFalse(self.cache_path.exists())
        with patch.object(network, "nmcli", return_value=subprocess.CompletedProcess([], 10, "", "")):
            self.assertEqual(network.query(["fixture"], missing_ok=True), [])
            with self.assertRaises(network.NetworkError):
                network.query(["fixture"])
        with patch.object(network, "nmcli", return_value=subprocess.CompletedProcess([], 8, "", "")):
            with self.assertRaises(network.NetworkError):
                network.query(["fixture"], missing_ok=True)

    def test_profile_cache_rejects_corrupt_stale_and_mismatched_data(self):
        saved = profile()
        inventory = [[saved["name"], saved["uuid"], "802-11-wireless"]]
        # Boolean timestamps would be fresh here if accepted as Python integers.
        now = 1
        cached = {"version": 1, "inventory": inventory, "updated": now, "profiles": [saved]}
        self.cache_path.write_text(json.dumps(cached))
        self.assertEqual(network.read_profile_cache(self.cache_path, inventory, now), [saved])
        cases = [
            ("invalid JSON", "not json"),
            ("wrong root type", "[]"),
            ("missing fields", '{"version":1}'),
            ("unsupported version", json.dumps({**cached, "version": 2})),
            ("changed inventory", json.dumps({**cached, "inventory": []})),
            ("missing profile", json.dumps({**cached, "profiles": []})),
            ("duplicate profile", json.dumps({**cached, "profiles": [saved, saved]})),
            ("wrong profiles type", json.dumps({**cached, "profiles": {}})),
            ("wrong profile type", json.dumps({**cached, "profiles": [None]})),
            *(
                (f"timestamp {value!r}", json.dumps({**cached, "updated": value}))
                for value in (True, False, "1", None, float("nan"), float("inf"), now + 1, now - 60)
            ),
            *(
                (f"invalid {field}", json.dumps({**cached, "profiles": [{**saved, field: value}]}))
                for field, value in (
                    ("name", "Renamed"), ("uuid", "other-uuid"), ("ssid", None),
                    ("device", []), ("bssid", 1), ("key", False), ("hidden", "false"),
                )
            ),
        ]
        for label, text in cases:
            with self.subTest(case=label):
                self.cache_path.write_text(text)
                self.assertIsNone(network.read_profile_cache(self.cache_path, inventory, now))

    def test_failed_cache_replace_preserves_previous_cache_and_cleans_staging_file(self):
        previous = '{"previous":"cache"}'
        self.cache_path.write_text(previous)
        with patch.object(network.os, "replace", side_effect=OSError("fixture failure")) as replace:
            network.write_profile_cache(self.cache_path, [], [], 100)
        replace.assert_called_once()
        self.assertEqual(self.cache_path.read_text(), previous)
        self.assertEqual(list(self.cache_path.parent.glob(".profiles-*")), [])

    def test_cache_write_failure_does_not_discard_discovered_profiles(self):
        inventory = [["Fixture", "uuid", "802-11-wireless"]]
        details = [["802-11-wireless.ssid", "Fixture"]]
        with patch.object(network, "query", side_effect=[inventory, details]), \
             patch.object(network.os, "replace", side_effect=OSError("fixture failure")) as replace:
            self.assertEqual(network.wifi_profiles(), [
                profile(name="Fixture", uuid="uuid", ssid="Fixture", key=""),
            ])
        replace.assert_called_once()
        self.assertFalse(self.cache_path.exists())
        self.assertEqual(list(self.cache_path.parent.glob(".profiles-*")), [])

    def test_unreadable_profile_cache_is_optional(self):
        self.cache_path.mkdir()
        with patch.object(network, "query", side_effect=[[], []]) as query:
            self.assertEqual(network.wifi_profiles(), [])
            self.assertEqual(network.wifi_profiles(), [])
        self.assertEqual(query.call_count, 2)
        self.assertTrue(self.cache_path.is_dir())
        self.assertEqual(list(self.cache_path.parent.glob(".profiles-*")), [])

    @patch("network_helper.subprocess.run")
    def test_nmcli_uses_literal_arguments_bounded_utf8_pipes_and_stable_locale(self, run):
        args = ["device", "wifi", "list", "ifname", "wlan 0; literal"]
        for stdin in (None, "synthetic-password\n"):
            with self.subTest(stdin=stdin):
                run.reset_mock()
                run.return_value = subprocess.CompletedProcess([], 4, "", "")
                with patch.dict(os.environ, {"LC_ALL": "other-locale", "FIXTURE": "preserved"}, clear=True):
                    result = network.nmcli(args, timeout=7, stdin=stdin)
                self.assertEqual(result.returncode, 4)
                run.assert_called_once_with(
                    ["nmcli", "--colors", "no", "--terse", "--escape", "yes", *args],
                    input=stdin,
                    stdin=subprocess.DEVNULL if stdin is None else None,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    encoding="utf-8",
                    errors="replace",
                    env={"LC_ALL": "C.UTF-8", "FIXTURE": "preserved"},
                    timeout=7,
                    check=False,
                )

    @patch("network_helper.subprocess.run")
    def test_failed_state_query_is_not_reported_as_disconnected(self, run):
        cases = [
            (8, "private-value", "Could not read NetworkManager state."),
            (0, "", "NetworkManager status is unavailable."),
            (0, "disconnected:none:enabled:enabled\n" * 2, "NetworkManager status is unavailable."),
            (0, "private-value:unsupported\n", "NetworkManager returned an unsupported record."),
            (0, "private-value\\", "NetworkManager returned an incomplete record."),
        ]
        for code, stdout, error in cases:
            with self.subTest(code=code, stdout=stdout):
                run.reset_mock()
                run.return_value = subprocess.CompletedProcess([], code, stdout, "private-value")
                result = self.run_main("", "status")
                self.assertFalse(result["ok"])
                self.assertIn(error, result["error"])
                self.assertNotIn("state", result)
                self.assertNotIn("private-value", repr(result))
                run.assert_called_once()

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

    def test_security_kinds_preserve_specific_wpa3_modes(self):
        for security, expected in (
            ("WPA1 WPA2", "personal"),
            ("WPA3", "personal"),
            ("WPA2 WPA3 802.1X", "enterprise"),
            ("WPA3 EAP", "enterprise"),
            ("WPA3 OWE", "owe"),
            ("WPA3 OWE-TM", "owe"),
            ("OWE", "owe"),
            ("WEP", "legacy"),
            ("--", "open"),
            ("", "open"),
            ("unknown", "unknown"),
        ):
            with self.subTest(security=security):
                self.assertEqual(network.security_kind(security), expected)

    def test_wpa3_owe_matches_saved_profile_without_password(self):
        entries = network.build_networks(
            [["", "Enhanced Open", "AA:BB", "90", "WPA3 OWE", "wlan0"]],
            [{"name": "wlan0", "managed": True}],
            [profile(ssid="Enhanced Open", key="owe")],
            {},
        )
        self.assertEqual(len(entries), 1)
        self.assertTrue(entries[0]["saved"])
        self.assertTrue(entries[0]["available"])
        self.assertFalse(entries[0]["protected"])
        self.assertFalse(entries[0]["advanced"])

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

    @patch("network_helper.subprocess.run")
    def test_nmcli_start_and_timeout_errors_are_explicit_and_sanitized(self, run):
        failures = [
            (FileNotFoundError("secret123"), "not installed"),
            (PermissionError("secret123"), "Check permissions"),
            (subprocess.TimeoutExpired(["nmcli"], 55, output="secret123", stderr="secret123"),
             "did not respond in time"),
        ]
        for failure, message in failures:
            with self.subTest(failure=type(failure).__name__):
                run.reset_mock()
                run.side_effect = failure
                result = self.run_main(json.dumps({
                    "operation": "connect", "network": access_point(), "password": "secret123",
                }))
                self.assertFalse(result["ok"])
                self.assertIn(message, result["error"])
                self.assertNotIn("secret123", repr(result))
                run.assert_called_once()
                self.assertEqual(run.call_args.kwargs["timeout"], 55)

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

    def test_hidden_access_point_and_bssid_shaped_ssid_remain_distinct(self):
        adapters = [{"name": "wlan0", "managed": True}]
        bssid = "AA:BB:CC:DD:EE:FF"
        aps = [
            ["", "", bssid, "20", "WPA2", "wlan0"],
            ["", bssid, "11:22:33:44:55:66", "90", "WPA2", "wlan0"],
        ]
        entries = network.build_networks(aps, adapters, [], {})
        self.assertEqual(len(entries), 2)
        self.assertEqual(len({entry["id"] for entry in entries}), 2)
        hidden = next(entry for entry in entries if entry["hidden"])
        visible = next(entry for entry in entries if not entry["hidden"])
        self.assertEqual(hidden["bssid"], bssid)
        self.assertTrue(hidden["requiresSsid"])
        self.assertEqual(visible["ssid"], bssid)
        self.assertFalse(visible["requiresSsid"])

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
    def test_saved_password_uses_octal_utf8_stdin_only(self, run):
        run.return_value = subprocess.CompletedProcess([], 0, "", "")
        password = " é\\"
        network.action({"operation": "connect", "network": access_point(uuid="saved-uuid"), "password": password})
        args = run.call_args.args[0]
        self.assertEqual(args[-2:], ["passwd-file", "/dev/stdin"])
        self.assertEqual(run.call_args.kwargs["stdin"], "802-11-wireless-security.psk:\\040\\303\\251\\134\n")
        self.assertNotIn(password, args)

    @unittest.skipUnless(sys.platform == "linux", "Requires native Linux GLib password-file decoding")
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
    def test_connection_exit_codes_control_retry_without_exposing_diagnostics(self, run):
        cases = [
            (0, ""),
            (3, "timed out"),
            (4, "Connection failed"),
            (7, "Check permissions"),
            (8, "not running"),
            (10, "no longer available"),
            (99, "could not complete"),
        ]
        for protected in (True, False):
            for code, message in cases:
                with self.subTest(protected=protected, code=code):
                    run.reset_mock()
                    run.return_value = subprocess.CompletedProcess([], code, "secret123", "secret123")
                    result = network.action({
                        "operation": "connect",
                        "network": access_point(uuid="saved-uuid", protected=protected),
                        "password": "secret123",
                    })
                    run.assert_called_once()
                    self.assertEqual(result["ok"], code == 0)
                    self.assertEqual(result["needsPassword"], protected and code in (4, 7))
                    if code:
                        self.assertIn(message, result["error"])
                    else:
                        self.assertEqual(result["error"], "")
                    self.assertNotIn("secret123", repr(result))

    @patch("network_helper.nmcli")
    def test_connection_invalidates_cache_before_and_after_success_or_failure(self, run):
        for code in (0, 4):
            with self.subTest(code=code):
                self.cache_path.write_text("before connection")
                def connect(*args, **kwargs):
                    self.assertFalse(self.cache_path.exists())
                    self.cache_path.write_text("status polled during connection")
                    return subprocess.CompletedProcess([], code, "", "")
                run.reset_mock()
                run.side_effect = connect
                result = network.action({
                    "operation": "connect", "network": access_point(uuid="saved-uuid"),
                })
                run.assert_called_once()
                self.assertEqual(result["ok"], code == 0)
                self.assertFalse(self.cache_path.exists())

    @patch("network_helper.nmcli")
    def test_wifi_toggle_does_not_disable_networking(self, run):
        for enabled in (False, True):
            for code in (0, 8):
                with self.subTest(enabled=enabled, code=code):
                    self.cache_path.write_text("previous cache")
                    run.reset_mock()
                    run.return_value = subprocess.CompletedProcess([], code, "private-value", "private-value")
                    result = network.action({"operation": "wifi", "enabled": enabled})
                    run.assert_called_once_with(["--wait", "10", "radio", "wifi", "on" if enabled else "off"])
                    self.assertEqual(result["ok"], code == 0)
                    self.assertEqual(bool(result["error"]), code != 0)
                    self.assertNotIn("private-value", repr(result))
                    self.assertFalse(self.cache_path.exists())

    @patch("network_helper.nmcli")
    @patch("network_helper.query")
    def test_rescan_without_usable_adapter_is_not_success(self, query, run):
        for devices in ([], [["eth0", "ethernet", "connected"]],
                        [["wlan0", "wifi", "unavailable"], ["wlan1", "wifi", "unmanaged"]]):
            with self.subTest(devices=devices):
                query.return_value = devices
                result = self.run_main('{"operation":"scan"}')
                self.assertFalse(result["ok"])
                self.assertIn("Wi-Fi adapter", result["error"])
        run.assert_not_called()

    @patch("network_helper.nmcli")
    @patch("network_helper.query")
    def test_rescan_uses_all_usable_adapters_and_reports_partial_failure(self, query, run):
        query.return_value = [
            ["eth0", "ethernet", "connected"],
            ["wlan0", "wifi", "connected"],
            ["wlan1", "wifi", "disconnected"],
            ["wlan2", "wifi", "unmanaged"],
            ["wlan3", "wifi", "unavailable"],
        ]
        for codes in ((0, 0), (4, 0), (0, 10)):
            with self.subTest(codes=codes):
                run.reset_mock()
                run.side_effect = [subprocess.CompletedProcess([], code, "", "") for code in codes]
                result = self.run_main('{"operation":"scan"}')
                self.assertEqual(result["ok"], not any(codes))
                self.assertEqual([call.args[0][-1] for call in run.call_args_list], ["wlan0", "wlan1"])
                if any(codes):
                    self.assertIn("wlan" + str(next(index for index, code in enumerate(codes) if code)),
                                  result["error"])
                    self.assertIn("Cached networks", result["error"])

    @patch("network_helper.subprocess.run")
    @patch("network_helper.query")
    def test_rescan_timeout_does_not_skip_other_adapters(self, query, run):
        query.return_value = [
            ["wlan0", "wifi", "connected"],
            ["wlan1", "wifi", "disconnected"],
        ]
        for failing_index in (0, 1):
            with self.subTest(failing_index=failing_index):
                run.reset_mock()
                results = [subprocess.CompletedProcess([], 0, "", "") for _ in range(2)]
                results[failing_index] = subprocess.TimeoutExpired(
                    ["nmcli"], 15, output="private-value", stderr="private-value"
                )
                run.side_effect = results
                result = self.run_main('{"operation":"scan"}')
                self.assertEqual(
                    [call.args[0][-1] for call in run.call_args_list],
                    ["wlan0", "wlan1"],
                )
                self.assertFalse(result["ok"])
                self.assertIn("wlan" + str(failing_index), result["error"])
                self.assertIn("did not respond in time", result["error"])
                self.assertIn("Cached networks", result["error"])
                self.assertNotIn("private-value", repr(result))

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
