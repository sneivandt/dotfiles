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
        self.settings = UNIT.read_text().splitlines()
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

    def stage(self, key):
        for line in self.settings:
            if not line.startswith(key + "="):
                continue
            program, *args = shlex.split(line.split("=", 1)[1])
            self.assertIn(program, ("/usr/bin/test", "/usr/bin/install", "/usr/bin/cmp", "/usr/bin/rm"))
            command = [program]
            for arg in args:
                if arg.startswith(("%h/", "%t/")):
                    path = self.expand(arg)
                    self.assertIn(path, (self.marker, self.snapshot))
                    command.append(str(path))
                else:
                    self.assertIn(arg, ("!", "-L", "-f", "-m", "0600", "-s", "--"))
                    command.append(arg)
            if subprocess.run(command, capture_output=True, check=False, timeout=5).returncode:
                return False
        return True

    def test_interrupted_or_superseded_work_is_retryable(self):
        self.marker.write_text('DOTFILES_PROFILE="fixture"\n')
        self.assertTrue(self.stage("ExecStartPre"))
        self.assertEqual(self.snapshot.read_bytes(), self.marker.read_bytes())
        self.assertEqual(self.snapshot.stat().st_mode & 0o777, 0o600)
        # A failed launcher has no ExecStartPost. A retry may see a new queue.
        self.assertTrue(self.stage("ExecStartPre"))
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
