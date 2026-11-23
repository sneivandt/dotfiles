"""Exercise launch argument handling without executing an application."""

import hashlib
import os
import shlex
import shutil
import subprocess
import tempfile
import time
import unittest

from script_test_support import BASH, SCRIPTS, ScriptFixture


MOCKS = r"""
function [ {
    if builtin [ "${MISSING_INSIDERS:-0}" = 1 ] &&
       builtin [ "$*" = "! -x /usr/bin/code-insiders ]" ]; then
        return 0
    fi
    builtin [ "$@"
}
command() {
    if [ "$1" = "-v" ]; then
        case " $AVAILABLE " in
            *" $2 "*)
                if [ "$2" = code-insiders ] && [ "${MISSING_INSIDERS:-0}" = 1 ]; then
                    printf '%s\n' "$HOME/.local/bin/code-insiders"
                fi
                return 0
                ;;
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
    if [ "$1" = "-v" ] && [ "$2" = "uwsm" ]; then
        return 0
    fi
    builtin command "$@"
}
logger() { printf 'LOGGER %s\n' "$*" >&2; return "${LOGGER_STATUS:-0}"; }
uwsm() {
    if [ "$1" = "check" ] && [ "$2" = "is-active" ]; then
        return "${UWSM_ACTIVE_STATUS:-0}"
    fi
    return 99
}
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
                *:1.10*) printf '{"type":"s","data":""}\n' ;;
                *:1.11*) return 1 ;;
                *) printf '{"type":"s","data":"%s"}\n' "${COPILOT_TITLE:-github}" ;;
            esac
            ;;
        *" Id")
            case "$*" in
                *:1.4*) printf '{"type":"s","data":"other-app"}\n' ;;
                *:1.10*) printf '{"type":"s","data":" Copilot "}\n' ;;
                *:1.11*) printf '{"type":"s","data":"GitHub Copilot"}\n' ;;
                *) printf '{"type":"s","data":"%s"}\n' "${COPILOT_ID:-ordinary}" ;;
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
    UWSM_PREFIX = [
        "uwsm", "app", "-t", "service", "-a", "github", "--",
        "env", "GDK_BACKEND=x11", "GTK_CSD=0",
    ]

    def launch(self, *args, **overrides):
        return self.run_script(
            "launch-github.sh",
            GITHUB_MOCKS,
            *args,
            env={
                "MISSING_APP": "0",
                "MISSING_TOOL": "",
                "UWSM_ACTIVE_STATUS": "0",
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

    def test_tray_matches_copilot_title_and_id_case_insensitively(self):
        cases = [
            ("copilot-title", '{"type":"as","data":[":1.9/tray"]}', {"COPILOT_TITLE": " Copilot "}),
            ("copilot-id-empty-title", '{"type":"as","data":[":1.10/tray"]}', {}),
            ("copilot-id-failed-title", '{"type":"as","data":[":1.11/tray"]}', {}),
        ]
        for label, registry, extra in cases:
            with self.subTest(case=label):
                (self.work / "calls").unlink(missing_ok=True)
                result = self.launch(REGISTRY=registry, **extra)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertIn("com.canonical.dbusmenu Event isvu 42 clicked i 0 0", self.calls())

    def test_cold_start_uses_deep_link_via_uwsm_service_or_fallback(self):
        for registry in (
            '{"type":"as","data":[]}',
            '{"type":"as","data":[":1.4/StatusNotifierItem"]}',
        ):
            with self.subTest(registry=registry, uwsm=True):
                result = self.launch(REGISTRY=registry)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(
                    result.stdout.split("\0")[:-1],
                    [*self.UWSM_PREFIX, "/usr/bin/github", "ghapp://mywork"],
                )
        with self.subTest(uwsm="missing"):
            result = self.launch(REGISTRY='{"type":"as","data":[]}', MISSING_TOOL="uwsm")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout.split("\0")[:-1],
                ["/usr/bin/github", "ghapp://mywork"],
            )
        with self.subTest(uwsm="inactive"):
            result = self.launch(REGISTRY='{"type":"as","data":[]}', UWSM_ACTIVE_STATUS="1")
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
            result.stdout.split("\0")[:-1],
            [*self.UWSM_PREFIX, "/usr/bin/github", "ghapp://mywork"],
        )

    def test_logging_failure_does_not_prevent_cold_start(self):
        result = self.launch(MISSING_TOOL="jq", LOGGER_STATUS="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("jq is unavailable", result.stderr)
        self.assertEqual(
            result.stdout.split("\0")[:-1],
            [*self.UWSM_PREFIX, "/usr/bin/github", "ghapp://mywork"],
        )

    def test_invalid_registry_is_an_explicit_failure(self):
        result = self.launch(REGISTRY='{"type":"as","data":null}')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("Invalid tray item response", result.stderr)

    def test_protocol_arguments_are_forwarded_without_window_recovery(self):
        args = [
            "ghapp://synthetic/path?q=space%20value&token=a$$b&ref=$HOME",
            "github-app://test",
            "",
        ]
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
                    [*self.UWSM_PREFIX, "/usr/bin/github", "ghapp://mywork"],
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
    def test_menu_and_protocol_launch_with_or_without_shared_helper(self):
        home = self.work / "home with spaces"
        launcher = home / ".config/hypr/scripts/launch-github.sh"
        launcher.parent.mkdir(parents=True)
        stub = (
            '#!/bin/sh\n'
            ': > "$HOME/argv"\n'
            'if [ "$#" -gt 0 ]; then printf "%s\\0" "$@" > "$HOME/argv"; fi\n'
            'printf "%s" "$#" > "$HOME/count"\n'
        )
        launcher.write_text(stub)
        launcher.chmod(0o700)
        native = self.work / "github"
        native.write_text(stub)
        native.chmod(0o700)
        desktop = self.work / "copilot.desktop"
        source = SCRIPTS.parents[2] / "local/share/applications/GitHub Copilot.desktop"
        contents = source.read_text()
        self.assertEqual(contents.count("/usr/bin/github"), 1)
        desktop.write_text(contents.replace("/usr/bin/github", str(native)))
        for helper, args in ((helper, args) for helper in (True, False)
                             for args in ([], ["ghapp://synthetic/path?q=space%20value"])):
            with self.subTest(helper=helper, args=args):
                if not helper:
                    launcher.unlink(missing_ok=True)
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


LEGACY_CODE_INSIDERS_SHIM = (
    b'#!/usr/bin/env python3\n'
    b'\n'
    b'import os\n'
    b'import sys\n'
    b'\n'
    b'REAL_BINARY = "/usr/bin/code-insiders"\n'
    b'PASSWORD_STORE = "--password-store=gnome-libsecret"\n'
    b'\n'
    b'\n'
    b'def normalized_args(arguments: list[str]) -> list[str]:\n'
    b'    forwarded: list[str] = []\n'
    b'    index = 0\n'
    b'\n'
    b'    while index < len(arguments):\n'
    b'        argument = arguments[index]\n'
    b'        if argument == "--":\n'
    b'            forwarded.extend(arguments[index:])\n'
    b'            break\n'
    b'        if argument == "--password-store":\n'
    b'            index += 1\n'
    b'            if index < len(arguments) and not arguments[index].startswith("-"):\n'
    b'                index += 1\n'
    b'            continue\n'
    b'        if argument.startswith("--password-store="):\n'
    b'            index += 1\n'
    b'            continue\n'
    b'\n'
    b'        forwarded.append(argument)\n'
    b'        index += 1\n'
    b'\n'
    b'    return [PASSWORD_STORE, *forwarded]\n'
    b'\n'
    b'\n'
    b'def main() -> None:\n'
    b'    os.execv(REAL_BINARY, [REAL_BINARY, *normalized_args(sys.argv[1:])])\n'
    b'\n'
    b'\n'
    b'if __name__ == "__main__":\n'
    b'    main()\n'
)


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class LauncherTests(ScriptFixture):
    def launch(self, name, args, available, cwd=None, missing_insiders=False):
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
                **self.env,
                "BASH_ENV": "",
                "ENV": "",
                "AVAILABLE": available,
                "MISSING_INSIDERS": "1" if missing_insiders else "0",
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

    def test_missing_insiders_backend_only_skips_the_managed_launcher(self):
        self.assertEqual(
            hashlib.sha256(LEGACY_CODE_INSIDERS_SHIM).hexdigest(),
            "abbafb0e03fa9c8534a5a74a46cbfc4357e73a34497da0966f96cddd6017eec9",
        )
        source = SCRIPTS.parents[2] / "local/bin/code-insiders"
        launcher = self.work / ".local/bin/code-insiders"
        launcher.parent.mkdir(parents=True)
        args = ["report 1.txt", "--wait", ""]
        for install, expected in [("copy", "code"), ("legacy-copy", "code"),
                                  ("link", "code"), ("custom", "code-insiders")]:
            with self.subTest(install=install):
                if install == "link":
                    if os.name == "nt":
                        continue  # Non-elevated Windows may not create symlinks.
                    launcher.symlink_to(source)
                elif install == "copy":
                    shutil.copyfile(source, launcher)
                elif install == "legacy-copy":
                    launcher.write_bytes(LEGACY_CODE_INSIDERS_SHIM)
                else:
                    launcher.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
                if not launcher.is_symlink():
                    launcher.chmod(0o755)
                result = self.launch("choose-editor.sh", args, "code-insiders code",
                                     missing_insiders=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.split("\0")[:-1], [expected, *args])
                launcher.unlink()

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
            for target in ("report.html", "file with spaces.html", "-local.html", "example.invalid", "youtube", "PRIME VIDEO"):
                with self.subTest(target=target):
                    with open(os.path.join(directory, target), "w", encoding="utf-8") as output:
                        output.write("fixture")
                    result = self.launch("choose-browser.sh", [target], "chromium", cwd=directory)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    filename = "./" + target if target.startswith("-") else target
                    self.assertEqual(result.stdout.split("\0")[:-1], ["chromium", filename])

    def test_missing_app_is_reported(self):
        for name in ("choose-editor.sh", "choose-browser.sh"):
            with self.subTest(name=name):
                result = self.launch(name, [], "")
                self.assertEqual(result.returncode, 1)
                self.assertIn("No supported", result.stderr)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
