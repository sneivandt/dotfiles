"""Run the real lock script with shell-only mocks; never start a locker or service."""

import os
from pathlib import Path
import shutil
import subprocess
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "lock-screen.sh"
BASH = shutil.which("bash")
MOCKS = r"""
exec 3>&2
pidof() { printf 'FOREIGN_PIDOF\n' >&3; printf '424242\n'; return 0; }
id() { printf '1000\n'; }
pgrep() {
    printf 'PGREP %s\n' "$*" >&3
    if builtin [ "$CURRENT_LOCKER" = 1 ]; then
        printf '12345\n'
        return 0
    fi
    return 1
}
systemctl() {
    printf 'SYSTEMCTL %s\n' "$*" >&3
    builtin [ "$ACTIVE_SERVICE" = 1 ]
}
systemd-run() { printf 'Unexpected direct launcher invocation\n' >&2; return 99; }
logger() { printf 'LOGGER %s\n' "$*" >&2; }
function [ {
    if builtin [ "$#" -eq 4 ] && builtin [ "$1" = "!" ] && builtin [ "$2" = "-x" ] && builtin [ "$3" = "/usr/bin/hyprlock" ]; then
        return 1
    fi
    builtin [ "$@"
}
exec() { printf '%s\n' "$@"; }
"""


@unittest.skipUnless(BASH, "Requires Bash (Git Bash on Windows)")
class LockScreenTests(unittest.TestCase):
    def run_script(self, current_locker=False, active_service=False):
        result = subprocess.run(
            [BASH, "--noprofile", "--norc", "-s"],
            input=MOCKS + SCRIPT.read_text(encoding="utf-8"),
            encoding="utf-8",
            capture_output=True,
            timeout=10,
            env={
                **os.environ,
                "BASH_ENV": "",
                "ENV": "",
                "CURRENT_LOCKER": str(int(current_locker)),
                "ACTIVE_SERVICE": str(int(active_service)),
            },
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PGREP -u 1000 -x hyprlock\n", result.stderr)
        self.assertNotIn("FOREIGN_PIDOF", result.stderr)
        return result

    def test_foreign_locker_does_not_suppress_current_user_lock(self):
        result = self.run_script()
        self.assertIn("SYSTEMCTL --user --quiet is-active hyprlock.service\n", result.stderr)
        self.assertEqual(
            result.stdout.splitlines(),
            [
                "systemd-run",
                "--user",
                "--quiet",
                "--collect",
                "--unit=hyprlock",
                "--property=Type=exec",
                "--property=NoNewPrivileges=no",
                "--property=Restart=on-abnormal",
                "--property=RestartSec=1",
                "/usr/bin/hyprlock",
                "--no-fade-in",
            ],
        )

    def test_current_user_locker_is_not_started_twice(self):
        result = self.run_script(current_locker=True)
        self.assertEqual(result.stdout, "")
        self.assertNotIn("SYSTEMCTL", result.stderr)

    def test_active_current_user_service_is_not_started_twice(self):
        result = self.run_script(active_service=True)
        self.assertIn("SYSTEMCTL --user --quiet is-active hyprlock.service\n", result.stderr)
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
