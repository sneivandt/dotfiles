"""Exercise media-key actions without touching audio or desktop notifications."""

import unittest

from script_test_support import BASH, ScriptFixture

MOCKS = r"""
command() {
    if [ "$1" = "-v" ] && [ "$2" = "notify-send" ]; then
        [ "${NO_NOTIFY:-0}" = 0 ]; return
    fi
    builtin command "$@"
}
wpctl() {
    printf '%s\n' "$*" >> "$HOME/calls"
    [ "${AUDIO_FAIL:-0}" = 0 ] || return 1
    if [ "$1" = get-volume ]; then
        printf '%s\n' "${VOLUME_STATE:-Volume: 0.55}"
    fi
}
notify-send() {
    printf '%s\n' "$@" > "$HOME/notification"
    return "${NOTIFY_STATUS:-0}"
}
"""


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class VolumeTests(ScriptFixture):
    def volume(self, action, **env):
        return self.run_script("volume.sh", MOCKS, action, env=env)

    def test_keyboard_adjustments_use_five_percent_steps_and_shared_limit(self):
        for action, step in (("up", "5%+"), ("down", "5%-")):
            with self.subTest(action=action):
                result = self.volume(action)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"set-volume --limit 1.5 @DEFAULT_AUDIO_SINK@ {step}",
                              (self.work / "calls").read_text())
                notification = (self.work / "notification").read_text()
                self.assertIn("Volume 55%", notification)
                self.assertIn("int:value:55", notification)

    def test_mute_targets_and_feedback(self):
        for action, target, label, tag in (
            ("mute", "SINK", "Volume", "volume"),
            ("mic-mute", "SOURCE", "Microphone", "microphone"),
        ):
            with self.subTest(action=action):
                result = self.volume(action, VOLUME_STATE="Volume: 0.55 [MUTED]")
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = (self.work / "calls").read_text()
                self.assertIn(f"set-mute @DEFAULT_AUDIO_{target}@ toggle", calls)
                self.assertIn(f"get-volume @DEFAULT_AUDIO_{target}@", calls)
                notification = (self.work / "notification").read_text()
                self.assertIn(f"{label} muted", notification)
                self.assertIn(f"synchronous:dotfiles-{tag}", notification)

    def test_unmuted_microphone_feedback(self):
        result = self.volume("mic-mute")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Microphone 55%", (self.work / "notification").read_text())

    def test_audio_failure_does_not_claim_success(self):
        result = self.volume("up", AUDIO_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.work / "notification").exists())

    def test_feedback_is_optional(self):
        for env in ({"NO_NOTIFY": "1"}, {"NOTIFY_STATUS": "1"}):
            with self.subTest(env=env):
                result = self.volume("up", **env)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_invalid_action_has_no_side_effects(self):
        result = self.volume("invalid")
        self.assertEqual(result.returncode, 2)
        self.assertFalse((self.work / "calls").exists())


if __name__ == "__main__":
    unittest.main()
