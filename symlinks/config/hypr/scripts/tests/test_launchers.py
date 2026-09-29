"""Exercise launch argument handling without executing an application."""

import os
import subprocess
import unittest

from script_test_support import BASH, SCRIPTS


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


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class LauncherTests(unittest.TestCase):
    def launch(self, name, args, available):
        return subprocess.run(
            [BASH, "--noprofile", "--norc", "-s", "--", *args],
            input=MOCKS + (SCRIPTS / name).read_text(encoding="utf-8"),
            capture_output=True,
            text=True,
            timeout=10,
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
        args = ["file with spaces.txt", "--wait", "", "another\nfile"]
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

    def test_missing_app_is_reported(self):
        for name in ("choose-editor.sh", "choose-browser.sh"):
            with self.subTest(name=name):
                result = self.launch(name, [], "")
                self.assertEqual(result.returncode, 1)
                self.assertIn("No supported", result.stderr)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
