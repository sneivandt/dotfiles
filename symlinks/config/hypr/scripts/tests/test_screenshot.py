"""Mock capture/clipboard commands; files stay inside a checkout-owned fixture."""

from concurrent.futures import ThreadPoolExecutor
import unittest

from script_test_support import BASH, ScriptFixture


MOCKS = r"""
command() {
    if [ "$1" = "-v" ]; then
        case "$2" in
            grim) [ "${NO_GRIM:-0}" = 0 ]; return ;;
            slurp) [ "${NO_SLURP:-0}" = 0 ]; return ;;
        esac
    fi
    builtin command "$@"
}
slurp() {
    [ "${CANCEL:-0}" = 0 ] || return 1
    : > "$HOME/selected"
    if [ "${COLLISION:-0}" = 1 ]; then
        mkdir -p "$HOME/screenshots"
        printf 'existing' > "$HOME/screenshots/fixture-timestamp-$$.png"
    fi
    printf '1,2 30x40\n'
}
date() {
    if [ "${NO_SLURP:-0}" = 0 ]; then
        [ -f "$HOME/selected" ] || return 1
    fi
    printf 'fixture-timestamp\n'
}
grim() {
    printf '%s\n' "$@" > "$HOME/arguments-$$"
    printf 'image' > "${@: -1}"
    [ "${CAPTURE_FAIL:-0}" = 0 ]
}
wl-copy() {
    cat > "$HOME/clipboard-$$"
    [ "${CLIPBOARD_FAIL:-0}" = 0 ]
}
"""


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class ScreenshotTests(ScriptFixture):
    def capture(self, **env):
        return self.run_script("screenshot.sh", MOCKS, env=env)

    def images(self):
        return list((self.work / "screenshots").glob("*.png"))

    def test_concurrent_same_second_captures_are_distinct(self):
        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(lambda _: self.capture(), range(2)))
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.images()), 2)
        for image in self.images():
            self.assertEqual(image.read_text(), "image")
        for arguments in self.work.glob("arguments-*"):
            self.assertEqual(arguments.read_text().splitlines()[:2], ["-g", "1,2 30x40"])

    def test_existing_destination_is_not_overwritten(self):
        result = self.capture(COLLISION="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([image.read_text() for image in self.images()], ["existing"])
        self.assertEqual(list(self.work.glob("arguments-*")), [])

    def test_cancellation_creates_no_destination(self):
        result = self.capture(CANCEL="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.work / "screenshots").exists())

    def test_failed_capture_removes_partial_image(self):
        result = self.capture(CAPTURE_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.images(), [])
        self.assertEqual(list(self.work.glob("clipboard-*")), [])

    def test_clipboard_failure_keeps_successful_capture(self):
        result = self.capture(CLIPBOARD_FAIL="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([image.read_text() for image in self.images()], ["image"])

    def test_fullscreen_fallback_without_slurp(self):
        result = self.capture(NO_SLURP="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        arguments = next(self.work.glob("arguments-*")).read_text().splitlines()
        self.assertEqual(len(arguments), 1)
        self.assertEqual([image.read_text() for image in self.images()], ["image"])

    def test_missing_grim_does_not_create_directory(self):
        result = self.capture(NO_GRIM="1")
        self.assertEqual(result.returncode, 1)
        self.assertIn("grim not found", result.stderr)
        self.assertFalse((self.work / "screenshots").exists())


if __name__ == "__main__":
    unittest.main()
