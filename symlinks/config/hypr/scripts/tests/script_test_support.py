"""Run shell fixtures in the checkout, never against the installed desktop."""

import os
from pathlib import Path
import shutil
import subprocess
import unittest
import uuid


SCRIPTS = Path(__file__).resolve().parents[1]
BASH = shutil.which("bash")


class ScriptFixture(unittest.TestCase):
    def setUp(self):
        self.work = Path.cwd() / f".hypr-script-fixture-{uuid.uuid4().hex}"
        self.work.mkdir(mode=0o700)
        self.addCleanup(shutil.rmtree, self.work)
        self.env = {
            **os.environ,
            "BASH_ENV": "",
            "ENV": "",
            "HOME": self.work.as_posix(),
            "XDG_CACHE_HOME": (self.work / "cache").as_posix(),
        }

    def run_script(self, name, mocks, *args, env=None):
        return subprocess.run(
            [BASH, "--noprofile", "--norc", "-s", "--", *args],
            input=mocks + "\n" + (SCRIPTS / name).read_text(encoding="utf-8"),
            text=True,
            capture_output=True,
            timeout=10,
            env={**self.env, **(env or {})},
        )
