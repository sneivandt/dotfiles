"""Native fixtures for the documentation validation entry point."""

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("docs_check", Path(__file__).resolve().parents[1] / "check.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


class CheckTests(unittest.TestCase):
    def test_native_npm_selection_and_missing_tool(self):
        for platform, name in (("win32", "npm.cmd"), ("linux", "npm"), ("darwin", "npm")):
            with self.subTest(platform=platform), patch.object(CHECKER.sys, "platform", platform):
                with patch.object(CHECKER.shutil, "which", return_value=name) as which:
                    self.assertEqual(CHECKER.npm_command(), name)
                    which.assert_called_once_with(name)
                with patch.object(CHECKER.shutil, "which", return_value=None):
                    with self.assertRaisesRegex(RuntimeError, "unavailable"):
                        CHECKER.npm_command()

    def test_checks_include_metadata_and_stop_before_browsers_on_failure(self):
        root = Path(tempfile.gettempdir()) / "docs fixture"
        with patch.object(CHECKER, "npm_command", return_value="npm"), patch.object(CHECKER, "run") as run:
            CHECKER.check(root, {})
            commands = [call.args[0] for call in run.call_args_list]
            self.assertEqual(len(commands), 8)
            self.assertIn("test-check-skills.py", commands[0][2])
            self.assertIn("check-skills.py", commands[1][2])
            self.assertIn("check-doc-links.py", commands[2][2])
            self.assertEqual(commands[3:6], [["npm", "run", "check"], ["npm", "test"], ["npm", "run", "build"]])
            self.assertIn("check-doc-site.py", commands[6][2])
            self.assertEqual(commands[7], ["npm", "run", "test:browser"])
            self.assertFalse(any(command[0] in {"sh", "wsl", "cargo"} for command in commands))
            run.reset_mock()
            run.side_effect = [None] * 6 + [subprocess.CalledProcessError(2, commands[6])]
            with self.assertRaises(subprocess.CalledProcessError):
                CHECKER.check(root, {})
            self.assertEqual(run.call_count, 7)

    def test_documentation_scope_excludes_embedded_docs_and_workflows(self):
        for path in ("README.md", "docs/check.py", "docs/content/testing.md", ".agents/skills/example/SKILL.md"):
            self.assertTrue(CHECKER.documentation_path(path), path)
        for path in ("cli/src/README.md", "conf/packages.toml", "docs-build/input", ".github/workflows/pages.yml"):
            self.assertFalse(CHECKER.documentation_path(path), path)


class CommittedTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="docs-check-fixture-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "repository with spaces"
        self.root.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith(("GIT_", "DOTFILES_"))}
        self.git("init", "--quiet", "--template=")
        self.git("config", "core.hooksPath", str(Path(self.temporary.name) / "no-hooks"))
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "user.name", "Documentation fixture")
        self.git("config", "user.email", "fixture@test.local")
        (self.root / "README.md").write_text("# Fixture\n", encoding="utf-8")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *arguments):
        return subprocess.check_output(
            ["git", "-C", str(self.root), *arguments], env=self.env, text=True, stderr=subprocess.PIPE,
        )

    def commit(self):
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Fixture inputs")

    def test_empty_and_invalid_ranges_fail_before_restore(self):
        with patch.object(CHECKER, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "empty"):
                CHECKER.check_committed(self.root, self.base, self.env)
            with self.assertRaises(subprocess.CalledProcessError):
                CHECKER.check_committed(self.root, "missing-fixture-ref", self.env)
            run.assert_not_called()

    def test_snapshot_uses_committed_checker_and_leaves_worktree_untouched(self):
        docs = self.root / "docs"
        docs.mkdir()
        checker = docs / "check.py"
        checker.write_text(
            "from pathlib import Path\n"
            "assert Path('docs/marker').read_text() == 'committed'\n"
            "assert not Path('untracked').exists()\n",
            encoding="utf-8",
        )
        marker = docs / "marker"
        marker.write_text("committed", encoding="utf-8")
        (self.root / "conf").mkdir()
        (self.root / "conf/packages.toml").write_text("", encoding="utf-8")
        self.commit()
        marker.write_text("staged", encoding="utf-8")
        self.git("add", "docs/marker")
        marker.write_text("working", encoding="utf-8")
        checker.write_text("raise RuntimeError('uncommitted checker')\n", encoding="utf-8")
        (self.root / "untracked").write_text("leave alone", encoding="utf-8")
        status = self.git("status", "--porcelain")
        head = self.git("rev-parse", "HEAD")
        calls = []
        real_run = CHECKER.run

        def run(command, cwd, env):
            calls.append((command, cwd))
            if command[0] != "fixture-npm":
                real_run(command, cwd, env)

        with patch.object(CHECKER, "npm_command", return_value="fixture-npm"), patch.object(CHECKER, "run", side_effect=run):
            CHECKER.check_committed(self.root, self.base, self.env)
        restore = next((command, cwd) for command, cwd in calls if command[0] == "fixture-npm")
        self.assertEqual(restore[0], ["fixture-npm", "ci", "--no-audit", "--no-fund"])
        self.assertNotEqual(restore[1], docs)
        self.assertFalse(restore[1].exists())
        self.assertEqual(calls[-1][0][0], sys.executable)
        self.assertEqual(self.git("status", "--porcelain"), status)
        self.assertEqual(self.git("rev-parse", "HEAD"), head)
        self.assertEqual(marker.read_text(encoding="utf-8"), "working")
        self.assertEqual(self.git("show", ":docs/marker"), "staged")


if __name__ == "__main__":
    unittest.main()
