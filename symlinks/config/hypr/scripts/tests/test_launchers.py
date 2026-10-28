"""Exercise launch argument handling without executing an application."""

import os
import shlex
import shutil
import subprocess
import tempfile
import time
import unittest

from script_test_support import BASH, SCRIPTS, ScriptFixture


MOCKS = r"""
command() {
    if [ "$1" = "-v" ]; then
        case " $AVAILABLE " in
            *" $2 "*) return 0 ;;
            *) return 1 ;;
        esac
    fi
    builtin command "$@"
}
exec() { printf '%s\0' "$@"; exit 0; }
"""

GITHUB_MOCKS = r"""
function [ {
    if builtin [ "$#" -eq 4 ] && builtin [ "$1" = "!" ] &&
       builtin [ "$2" = "-x" ] && builtin [ "$3" = "/usr/bin/github" ]; then
        builtin [ "$MISSING_APP" = 1 ]
        return
    fi
    builtin [ "$@"
}
command() {
    if [ "$1" = "-v" ] && [ "$2" = "$MISSING_TOOL" ]; then
        return 1
    fi
    builtin command "$@"
}
logger() { printf 'LOGGER %s\n' "$*" >&2; }
exec() { printf '%s\0' "$@"; exit 0; }
hyprctl() {
    printf 'Unexpected compositor action\n' >&2
    return 99
}
busctl() {
    printf 'BUSCTL %s\n' "$*" >> "$HOME/calls"
    case "$*" in
        *RegisteredStatusNotifierItems)
            [ "$FAIL_STAGE" != registry ] || return 1
            printf '%s\n' "$REGISTRY"
            ;;
        *Title)
            case "$*" in
                *:1.4*) printf '{"type":"s","data":"Other App"}\n' ;;
                *) printf '{"type":"s","data":"github"}\n' ;;
            esac
            ;;
        *" Menu")
            [ "$FAIL_STAGE" != menu ] || return 1
            printf '{"type":"o","data":"/tray/Menu"}\n'
            ;;
        *GetLayout*)
            [ "$FAIL_STAGE" != layout ] || return 1
            printf '%s\n' "$LAYOUT"
            ;;
        *Event*)
            [ "$FAIL_STAGE" != event ]
            ;;
        *) return 99 ;;
    esac
}
"""


@unittest.skipUnless(BASH and shutil.which("jq"), "Requires Bash and jq")
class GitHubLauncherTests(ScriptFixture):
    def launch(self, *args, **overrides):
        return self.run_script(
            "launch-github.sh",
            GITHUB_MOCKS,
            *args,
            env={
                "MISSING_APP": "0",
                "MISSING_TOOL": "",
                "FAIL_STAGE": "",
                "REGISTRY": '{"type":"as","data":[":1.4/StatusNotifierItem",":1.9/tray"]}',
                "LAYOUT": (
                    '{"type":"u(ia{sv}av)","data":[1,[0,{},['
                    '{"type":"(ia{sv}av)","data":[42,'
                    '{"label":{"type":"s","data":"Open GitHub Copilot"}},[]]}]]]}'
                ),
                **overrides,
            },
        )

    def calls(self):
        path = self.work / "calls"
        return path.read_text() if path.exists() else ""

    def test_tray_only_instance_uses_dynamic_menu_action_without_relaunch(self):
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        calls = self.calls()
        self.assertIn("GetLayout iias 0 1 1 label", calls)
        self.assertIn(
            ":1.9 /tray/Menu com.canonical.dbusmenu Event isvu 42 clicked i 0 0",
            calls,
        )
        self.assertNotIn("Activate", calls)
        self.assertNotIn(":1.4 /StatusNotifierItem org.kde.StatusNotifierItem Menu", calls)

    def test_cold_start_uses_deep_link(self):
        for registry in (
            '{"type":"as","data":[]}',
            '{"type":"as","data":[":1.4/StatusNotifierItem"]}',
        ):
            with self.subTest(registry=registry):
                result = self.launch(REGISTRY=registry)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(
                    result.stdout.split("\0")[:-1],
                    ["/usr/bin/github", "ghapp://mywork"],
                )

    def test_missing_watcher_reports_fallback(self):
        result = self.launch(FAIL_STAGE="registry")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Could not query tray items", result.stderr)
        self.assertEqual(
            result.stdout.split("\0")[:-1], ["/usr/bin/github", "ghapp://mywork"]
        )

    def test_invalid_registry_is_an_explicit_failure(self):
        result = self.launch(REGISTRY='{"type":"as","data":null}')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("Invalid tray item response", result.stderr)

    def test_protocol_arguments_are_forwarded_without_window_recovery(self):
        args = ["ghapp://synthetic/path?q=space%20value", "github-app://test", ""]
        result = self.launch(*args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split("\0")[:-1], ["/usr/bin/github", *args])
        self.assertEqual(self.calls(), "")

    def test_service_only_registration_uses_default_path(self):
        result = self.launch(REGISTRY='{"type":"as","data":["org.example.Copilot"]}')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "org.example.Copilot /StatusNotifierItem org.kde.StatusNotifierItem Menu",
            self.calls(),
        )

    def test_missing_recovery_tools_report_fallback(self):
        for tool in ("jq", "busctl"):
            with self.subTest(tool=tool):
                result = self.launch(MISSING_TOOL=tool)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"{tool} is unavailable", result.stderr)
                self.assertEqual(
                    result.stdout.split("\0")[:-1],
                    ["/usr/bin/github", "ghapp://mywork"],
                )

    def test_tray_failures_are_not_reported_as_success(self):
        for stage in ("menu", "layout", "event"):
            with self.subTest(stage=stage):
                result = self.launch(FAIL_STAGE=stage)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, "")
                self.assertIn("Could not", result.stderr)

    def test_missing_tray_action_is_reported(self):
        result = self.launch(LAYOUT='{"type":"u(ia{sv}av)","data":[1,[0,{},[]]]}')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("Could not find", result.stderr)
        self.assertNotIn("Event", self.calls())

    def test_missing_app_is_reported(self):
        result = self.launch(MISSING_APP="1")
        self.assertEqual(result.returncode, 1)
        self.assertIn("GitHub Copilot is not installed", result.stderr)
        self.assertEqual(self.calls(), "")


@unittest.skipUnless(shutil.which("gio") and BASH, "Requires Gio and Bash")
class GitHubDesktopEntryTests(ScriptFixture):
    def test_menu_launch_and_protocol_uri_use_shared_launcher(self):
        home = self.work / "home with spaces"
        launcher = home / ".config/hypr/scripts/launch-github.sh"
        launcher.parent.mkdir(parents=True)
        launcher.write_text(
            '#!/bin/sh\n'
            ': > "$HOME/argv"\n'
            'if [ "$#" -gt 0 ]; then printf "%s\\0" "$@" > "$HOME/argv"; fi\n'
            'printf "%s" "$#" > "$HOME/count"\n'
        )
        launcher.chmod(0o700)
        desktop = self.work / "copilot.desktop"
        source = SCRIPTS.parents[2] / "local/share/applications/GitHub Copilot.desktop"
        desktop.write_text(source.read_text())
        for args in ([], ["ghapp://synthetic/path?q=space%20value"]):
            with self.subTest(args=args):
                (home / "count").unlink(missing_ok=True)
                result = subprocess.run(
                    ["gio", "launch", str(desktop), *args],
                    env={**self.env, "HOME": str(home)},
                    capture_output=True,
                    text=True,
                    timeout=10,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                for _ in range(50):
                    if (home / "count").exists():
                        break
                    time.sleep(0.1)
                self.assertTrue((home / "count").exists(), result.stderr)
                self.assertEqual((home / "count").read_text(), str(len(args)))
                actual = (home / "argv").read_bytes().decode()
                self.assertEqual(actual.split("\0")[:-1] if actual else [], args)


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class LauncherTests(unittest.TestCase):
    def launch(self, name, args, available, cwd=None):
        # Python's Windows argv quoting does not protect literal newlines from
        # Git Bash. Set the fixture arguments inside Bash instead.
        script = MOCKS + "set -- " + shlex.join(args) + "\n"
        script += (SCRIPTS / name).read_text(encoding="utf-8")
        return subprocess.run(
            [BASH, "--noprofile", "--norc", "-s"],
            input=script,
            capture_output=True,
            text=True,
            timeout=10,
            cwd=cwd,
            env={
                **os.environ,
                "BASH_ENV": "",
                "ENV": "",
                "AVAILABLE": available,
            },
        )

    def assert_launch(self, name, args, available, expected):
        result = self.launch(name, args, available)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split("\0")[:-1], expected)

    def test_editor_preserves_arguments_and_preference(self):
        args = ["file with spaces.txt", "--wait", "", "another\nfile", "'quoted' $literal"]
        for editor in ("code-insiders", "code", "gvim"):
            with self.subTest(editor=editor):
                self.assert_launch(
                    "choose-editor.sh", args, editor, [editor, *args]
                )
        self.assert_launch(
            "choose-editor.sh", [], "code-insiders code gvim", ["code-insiders"]
        )

    def test_browser_aliases_still_open_apps(self):
        cases = [
            (["Prime", "Video"], "https://amazon.com/video"),
            (["PRIME VIDEO"], "https://amazon.com/video"),
            (["ChatGPT"], "https://chat.openai.com"),
            (["lichess"], "https://lichess.org"),
            (["netflix"], "https://netflix.com"),
            (["youtube"], "https://youtube.com/"),
        ]
        for args, url in cases:
            with self.subTest(args=args):
                self.assert_launch(
                    "choose-browser.sh", args, "chromium", ["chromium", f"--app={url}"]
                )

    def test_browser_preserves_general_argv(self):
        cases = [
            [],
            [""],
            ["https://example.invalid/one", "https://example.invalid/two"],
            ["--incognito", "https://example.invalid/path with spaces"],
            ["--user-data-dir=./profile.v1"],
            ["./file with spaces.html"],
            ["mailto:test@example.invalid"],
            ["prime", "video", "--incognito"],
        ]
        for args in cases:
            with self.subTest(args=args):
                self.assert_launch(
                    "choose-browser.sh", args, "chromium-dev chromium",
                    ["chromium-dev", *args],
                )

    def test_browser_single_web_target_still_opens_app(self):
        for target, url in [
            ("https://example.invalid/a", "https://example.invalid/a"),
            ("example.invalid", "https://example.invalid"),
        ]:
            with self.subTest(target=target):
                self.assert_launch(
                    "choose-browser.sh", [target], "chromium", ["chromium", f"--app={url}"]
                )

    def test_browser_existing_relative_files_are_not_hostnames(self):
        with tempfile.TemporaryDirectory() as directory:
            for target in ("report.html", "file with spaces.html", "example.invalid"):
                with self.subTest(target=target):
                    with open(os.path.join(directory, target), "w", encoding="utf-8") as output:
                        output.write("fixture")
                    result = self.launch("choose-browser.sh", [target], "chromium", cwd=directory)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout.split("\0")[:-1], ["chromium", target])

    def test_missing_app_is_reported(self):
        for name in ("choose-editor.sh", "choose-browser.sh"):
            with self.subTest(name=name):
                result = self.launch(name, [], "")
                self.assertEqual(result.returncode, 1)
                self.assertIn("No supported", result.stderr)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
