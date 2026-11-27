"""Exercise brightness key actions without touching hardware backlights or notifications."""

import unittest

from script_test_support import BASH, ScriptFixture

MOCKS = r"""
command() {
    if [ "$1" = "-v" ] && [ "$2" = "quickshell" ]; then
        [ "${HAVE_QUICKSHELL:-0}" = 1 ]; return
    fi
    if [ "$1" = "-v" ] && [ "$2" = "notify-send" ]; then
        [ "${NO_NOTIFY:-0}" = 0 ]; return
    fi
    builtin command "$@"
}
brightnessctl() {
    printf '%s\n' "$*" >> "$HOME/calls"
    case "$*" in
        *" set "*)
            [ "${SET_FAIL:-0}" = 0 ] || return 1
            ;;
        *" get")
            [ "${READBACK_FAIL:-0}" = 0 ] || return 1
            printf '%s\n' "${BRIGHTNESS_CUR:-600}"
            ;;
        *" max")
            [ "${READBACK_FAIL:-0}" = 0 ] || return 1
            printf '%s\n' "${BRIGHTNESS_MAX:-1000}"
            ;;
        *) return 99 ;;
    esac
}
quickshell() {
    printf '%s\n' "$*" > "$HOME/osd"
    return "${QUICKSHELL_STATUS:-0}"
}
notify-send() {
    printf '%s\n' "$@" > "$HOME/notification"
    return "${NOTIFY_STATUS:-0}"
}
"""


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class BrightnessTests(ScriptFixture):
    def setUp(self):
        super().setUp()
        self.backlight = self.work / "backlight"
        (self.backlight / "intel_backlight").mkdir(parents=True)

    def brightness(self, action, backlight_dir=None, **env):
        return self.run_script(
            "brightness.sh",
            MOCKS,
            action,
            env={"BACKLIGHT_DIR": str(backlight_dir or self.backlight), **env},
        )

    def test_adjustments_quiet_brightnessctl_and_send_progress_toast(self):
        for action, step in (("up", "+5%"), ("down", "5%-")):
            with self.subTest(action=action):
                (self.work / "calls").unlink(missing_ok=True)
                (self.work / "notification").unlink(missing_ok=True)
                result = self.brightness(action)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "")
                calls = (self.work / "calls").read_text()
                self.assertIn(f"--class=backlight --quiet --min-value=1 set {step}", calls)
                notification = (self.work / "notification").read_text()
                self.assertIn("--category=progress", notification)
                self.assertIn("synchronous:dotfiles-brightness", notification)
                self.assertIn("int:value:60", notification)
                self.assertIn("Brightness 60%", notification)

    def test_desktop_without_backlight_is_a_silent_noop(self):
        empty = self.work / "empty-backlight"
        empty.mkdir()
        result = self.brightness("up", backlight_dir=empty)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.work / "calls").exists())
        self.assertFalse((self.work / "notification").exists())

    def test_brightness_failure_does_not_claim_success(self):
        result = self.brightness("up", SET_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.work / "notification").exists())

    def test_readback_or_notification_failure_keeps_successful_change(self):
        for env in ({"READBACK_FAIL": "1"}, {"NO_NOTIFY": "1"}, {"NOTIFY_STATUS": "1"}):
            with self.subTest(env=env):
                result = self.brightness("up", **env)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_quickshell_osd_suppresses_duplicate_notification_and_falls_back(self):
        result = self.brightness("up", HAVE_QUICKSHELL="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            (self.work / "osd").read_text().strip(), "ipc call osd display brightness 60 false"
        )
        self.assertFalse((self.work / "notification").exists())

        result = self.brightness("up", HAVE_QUICKSHELL="1", QUICKSHELL_STATUS="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Brightness 60%", (self.work / "notification").read_text())

    def test_invalid_action_has_no_side_effects(self):
        result = self.brightness("invalid")
        self.assertEqual(result.returncode, 2)
        self.assertFalse((self.work / "calls").exists())


if __name__ == "__main__":
    unittest.main()
