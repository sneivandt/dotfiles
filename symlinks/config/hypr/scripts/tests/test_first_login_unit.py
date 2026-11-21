"""Exercise the shipped queue handoff commands, never systemd or the desktop."""

import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import tomllib
import unittest


UNIT = Path(__file__).resolve().parents[3] / "systemd/user/dotfiles-first-login.service"


class GraphicalUnitTests(unittest.TestCase):
    def test_graphical_declarations_also_follow_the_session_on_next_login(self):
        declarations = UNIT.parents[4] / "conf/systemd-units.toml"
        groups = tomllib.loads(declarations.read_text()).values()
        for group in groups:
            for entry in group["units"]:
                if not isinstance(entry, dict) or not entry.get("graphical") or not entry.get("enabled", True):
                    continue
                unit = UNIT.with_name(entry["name"])
                if not unit.exists():  # packaged unit, not a shipped definition
                    continue
                with self.subTest(unit=unit.name):
                    lines = unit.read_text().splitlines()
                    targets = {target for line in lines if line.startswith(("WantedBy=", "RequiredBy="))
                               for target in line.split("=", 1)[1].split()}
                    self.assertEqual(targets, {"graphical-session.target"})
                    self.assertIn("PartOf=graphical-session.target", lines)


@unittest.skipUnless(sys.platform == "linux", "the unit uses Linux coreutils")
class FirstLoginTests(unittest.TestCase):
    def setUp(self):
        fixture = tempfile.TemporaryDirectory(dir=Path.cwd(), prefix=".first-login-")
        self.addCleanup(fixture.cleanup)
        root = Path(fixture.name)
        self.home = root / "home with spaces"
        self.runtime = root / "runtime"
        self.settings = UNIT.read_text().replace("\\\n", " ").splitlines()
        self.marker = self.expand(self.value("ConditionPathExists"))
        self.snapshot = self.expand(self.value("EnvironmentFile").removeprefix("-"))
        self.assertTrue(self.marker.resolve().is_relative_to(self.home))
        self.assertTrue(self.snapshot.resolve().is_relative_to(self.runtime))
        self.assertEqual(self.snapshot.parent, self.runtime / self.value("RuntimeDirectory"))
        self.marker.parent.mkdir(parents=True)
        self.snapshot.parent.mkdir(parents=True)

    def value(self, key):
        value, = (line.split("=", 1)[1] for line in self.settings if line.startswith(key + "="))
        return value

    def expand(self, arg):
        return Path(arg.replace("%h", str(self.home)).replace("%t", str(self.runtime)))

    def stage(self, key, *, rm="/usr/bin/rm"):
        return self.run_stage(key, rm=rm).returncode == 0

    def run_stage(self, key, *, rm="/usr/bin/rm"):
        env = {"HOME": str(self.home), "PATH": "/usr/bin:/bin", "FIXTURE_RM": str(rm)}
        result = None
        for line in self.settings:
            if not line.startswith(key + "="):
                continue
            program, *args = shlex.split(line.split("=", 1)[1])
            if program == ":/bin/sh":
                flags, script, name, marker, snapshot, directory = args
                self.assertEqual((flags, name), ("-ec", "sh"))
                self.assertEqual((self.expand(marker), self.expand(snapshot), self.expand(directory)),
                                 (self.marker, self.snapshot, self.marker.parent))
                for step in script.split("; "):
                    self.assertIn(step.strip(), (
                        'exec 9<"$3"', '/usr/bin/flock --nonblock --conflict-exit-code 75 9 || { rc=$?',
                        'test "$rc" -ne 75 || echo "Deferred extensions queue is busy. The service will retry." >&2',
                        'exit "$rc"', '}', 'test ! -L "$1"', 'test -f "$1"',
                        '/usr/bin/cmp -s -- "$2" "$1"', '/usr/bin/rm -- "$1"',
                    ))
                command = ["/bin/sh", flags, script.replace("/usr/bin/rm", '"$FIXTURE_RM"'), name,
                           self.marker, self.snapshot, self.expand(directory)]
                args = []
            else:
                self.assertIn(program, ("/usr/bin/test", "/usr/bin/install", "/usr/bin/cmp", "/usr/bin/rm"))
                command = [rm if program == "/usr/bin/rm" else program]
            for arg in args:
                if arg.startswith(("%h/", "%t/")):
                    path = self.expand(arg)
                    self.assertIn(path, (self.marker, self.snapshot))
                    command.append(str(path))
                else:
                    self.assertIn(arg, ("!", "-L", "-f", "-m", "0600", "-s", "--"))
                    command.append(arg)
            result = subprocess.run(command, env=env, capture_output=True, check=False, timeout=5)
            if result.returncode:
                break
        self.assertIsNotNone(result, f"unit has no {key} commands")
        return result

    def test_busy_queue_fails_without_consuming_then_retries(self):
        import fcntl

        queued = b'DOTFILES_PROFILE="fixture"\n'
        self.marker.write_bytes(queued)
        self.assertTrue(self.stage("ExecStartPre"))
        holder = os.open(self.marker.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            fcntl.flock(holder, fcntl.LOCK_EX | fcntl.LOCK_NB)
            result = self.run_stage("ExecStartPost")
            self.assertEqual(result.returncode, 75)
            self.assertIn(b"queue is busy. The service will retry", result.stderr)
            self.assertEqual(self.marker.read_bytes(), queued)
        finally:
            os.close(holder)
        self.assertTrue(self.stage("ExecStartPre"))
        self.assertEqual(self.snapshot.read_bytes(), queued)
        self.assertTrue(self.stage("ExecStartPost"))
        self.assertFalse(self.marker.exists())

    def test_replacement_between_comparison_and_unlink_survives(self):
        self.marker.write_text('DOTFILES_PROFILE="old"\n')
        self.assertTrue(self.stage("ExecStartPre"))
        replacement = b'DOTFILES_PROFILE="new"\n'
        pending = self.marker.with_suffix(".next")
        pending.write_bytes(replacement)

        # At the actual unlink boundary, let a cooperating producer publish if
        # it can. The old split post commands let mv win, then delete its work.
        rm = self.home / "race-at-unlink"
        rm.write_text(
            '#!/bin/sh\nset -eu\nmarker=$2\n'
            '/usr/bin/flock -n -E 42 "${marker%/*}" /usr/bin/mv -- "$marker.next" "$marker"'
            ' || test "$?" = 42\nexec /usr/bin/rm "$@"\n'
        )
        rm.chmod(0o700)
        self.assertTrue(self.stage("ExecStartPost", rm=rm))
        if pending.exists():  # Lock released; the producer can retry.
            pending.replace(self.marker)
        self.assertTrue(self.marker.exists(), "acknowledgement deleted the replacement queue")
        self.assertEqual(self.marker.read_bytes(), replacement)
        self.assertTrue(self.stage("ExecStartPre"))
        self.assertEqual(self.snapshot.read_bytes(), replacement)
        self.assertTrue(self.stage("ExecStartPost"))
        self.assertFalse(self.marker.exists())

    def test_interrupted_or_superseded_work_is_retryable(self):
        self.marker.write_text('DOTFILES_PROFILE="fixture"\n')
        self.assertTrue(self.stage("ExecStartPre"))
        self.assertEqual(self.snapshot.read_bytes(), self.marker.read_bytes())
        self.assertEqual(self.snapshot.stat().st_mode & 0o777, 0o600)
        # A failed launcher has no ExecStartPost. A retry may see a new queue.
        self.assertTrue(self.stage("ExecStartPre"))
        self.assertFalse(self.stage("ExecStartPost", rm="/usr/bin/false"))
        self.assertEqual(self.marker.read_bytes(), self.snapshot.read_bytes())
        self.marker.write_text('DOTFILES_PROFILE="new-selection"\n')
        self.assertFalse(self.stage("ExecStartPost"))
        self.assertNotEqual(self.snapshot.read_bytes(), self.marker.read_bytes())
        self.assertTrue(self.stage("ExecStartPre"))
        self.assertEqual(self.snapshot.read_bytes(), self.marker.read_bytes())
        self.assertTrue(self.stage("ExecStartPost"))
        self.assertFalse(self.marker.exists())

    def test_replaced_paths_are_never_consumed(self):
        # An empty regular marker from an older installation is valid.
        self.marker.touch()
        self.assertTrue(self.stage("ExecStartPre"))
        self.marker.unlink()
        target = self.home / "user-file"
        target.touch()
        self.marker.symlink_to(target)
        self.assertFalse(self.stage("ExecStartPost"))
        self.assertFalse(self.stage("ExecStartPre"))
        self.assertTrue(self.marker.is_symlink())
        self.assertTrue(target.exists())
        self.marker.unlink()
        os.mkfifo(self.marker)
        self.assertFalse(self.stage("ExecStartPre"))
