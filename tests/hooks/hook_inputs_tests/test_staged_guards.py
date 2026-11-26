"""Staged-input regression tests for hooks/check-ci-guards.sh and hooks/check-rust.sh."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

from .helpers import REPO_ROOT, init_fixture_repository, run, write_executable


class StagedGuardTests(unittest.TestCase):
    def test_fixture_git_context(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            caller = fixture / "caller"
            child = fixture / "child"

            caller_env = init_fixture_repository(caller)
            (caller / "marker").write_text("caller\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "marker"], cwd=caller, env=caller_env).returncode, 0)
            caller_tree = run(["git", "write-tree"], cwd=caller, env=caller_env).stdout.strip()

            child_env = init_fixture_repository(child, base_env=caller_env)
            (child / "marker").write_text("child\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "marker"], cwd=child, env=child_env).returncode, 0)
            self.assertEqual(run(["git", "show", ":marker"], cwd=child, env=child_env).stdout.strip(), "child")

            self.assertEqual(run(["git", "write-tree"], cwd=caller, env=caller_env).stdout.strip(), caller_tree)

    def test_staged_ci_guards(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            mock_bin = fixture / "mock-bin"
            repo = fixture / "repo [literal]"
            for sub in ("hooks", "cli", "conf"):
                (repo / sub).mkdir(parents=True)

            write_executable(
                mock_bin / "shellcheck",
                "#!/bin/sh\n"
                "set -eu\n"
                "for file do\n"
                '  case "$file" in --*) continue ;; esac\n'
                '  [ "$(cat "$file")" = valid-staged ]\n'
                "done\n"
                'printf "shellcheck\\n" >> "$HOOK_INPUT_LOG"\n',
            )
            write_executable(
                mock_bin / "cargo",
                "#!/bin/sh\n"
                "set -eu\n"
                '[ "$1" = run ]\n'
                '[ -n "${DOTFILES_VERSION:-}" ]\n'
                'case "${CARGO_TARGET_DIR:-}" in\n'
                "  */cli/target/hooks) ;;\n"
                "  *) exit 1 ;;\n"
                "esac\n"
                'while [ "$#" -gt 0 ]; do\n'
                '  if [ "$1" = --root ]; then\n'
                "    shift\n"
                '    case "$1" in */.git/*) exit 1 ;; esac\n'
                '    [ "$(cat "$1/conf/fixture.toml")" = staged-config ]\n'
                '    [ ! -e "$1/conf/untracked.toml" ]\n'
                '    printf "config\\n" >> "$HOOK_INPUT_LOG"\n'
                "    exit 0\n"
                "  fi\n"
                "  shift\n"
                "done\n"
                "exit 1\n",
            )

            log_file = fixture / "checks.log"
            env = init_fixture_repository(repo)
            env.pop("CARGO_TARGET_DIR", None)
            env.pop("DOTFILES_VERSION", None)
            env["PATH"] = f"{mock_bin}{os.pathsep}{env.get('PATH', '')}"
            env["HOOK_INPUT_LOG"] = str(log_file)

            (repo / "hooks" / "script with spaces.sh").write_text("valid-staged\n", encoding="utf-8")
            (repo / "cli" / "Cargo.toml").write_text('[dependencies]\nexample = "1"\n', encoding="utf-8")
            (repo / "conf" / "fixture.toml").write_text("staged-config\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "hooks", "cli", "conf"], cwd=repo, env=env).returncode, 0)

            (repo / "hooks" / "script with spaces.sh").write_text("invalid-unstaged\n", encoding="utf-8")
            (repo / "conf" / "fixture.toml").write_text("unstaged-config\n", encoding="utf-8")
            (repo / "conf" / "untracked.toml").write_text("untracked-config\n", encoding="utf-8")
            res = run(["sh", str(REPO_ROOT / "hooks" / "check-ci-guards.sh")], cwd=repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["config", "shellcheck"])

            (repo / "hooks" / "script with spaces.sh").write_text("invalid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "hooks"], cwd=repo, env=env).returncode, 0)
            (repo / "hooks" / "script with spaces.sh").write_text("valid-staged\n", encoding="utf-8")
            res = run(["sh", str(REPO_ROOT / "hooks" / "check-ci-guards.sh")], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0, "CI guards accepted invalid staged shell content")
            self.assertIn("ShellCheck reported issues", res.stdout + res.stderr)

            self.assertEqual(run(["git", "add", "hooks"], cwd=repo, env=env).returncode, 0)
            (repo / "hooks" / "script with spaces.sh").unlink()
            (repo / "conf" / "fixture.toml").write_text("staged-config\n", encoding="utf-8")
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", str(REPO_ROOT / "hooks" / "check-ci-guards.sh")], cwd=repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["config", "shellcheck"])

            (repo / "conf" / "fixture.toml").write_text("invalid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "conf/fixture.toml"], cwd=repo, env=env).returncode, 0)
            (repo / "conf" / "fixture.toml").write_text("staged-config\n", encoding="utf-8")
            res = run(["sh", str(REPO_ROOT / "hooks" / "check-ci-guards.sh")], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0, "CI guards accepted invalid staged config")
            self.assertIn("configuration validation failed", res.stdout + res.stderr)
            self.assertEqual(run(["git", "add", "conf/fixture.toml"], cwd=repo, env=env).returncode, 0)

            (repo / "cli" / "Cargo.toml").write_text('[dependencies]\nexample = "*"\n', encoding="utf-8")
            self.assertEqual(run(["git", "add", "cli"], cwd=repo, env=env).returncode, 0)
            (repo / "cli" / "Cargo.toml").write_text('[dependencies]\nexample = "1"\n', encoding="utf-8")
            res = run(["sh", str(REPO_ROOT / "hooks" / "check-ci-guards.sh")], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0, "CI guards accepted a staged wildcard dependency")
            self.assertIn("wildcard dependency", res.stdout + res.stderr)

    def test_staged_quoted_shell(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            repo = fixture / "repo"
            mock_bin = fixture / "mock-bin"
            (repo / "hooks").mkdir(parents=True)
            (repo / "cli" / "src" / "app" / "validation" / "scripts").mkdir(parents=True)
            shutil.copy2(REPO_ROOT / "hooks" / "check-ci-guards.sh", repo / "hooks" / "check-ci-guards.sh")
            # Isolate the hook's selection and forwarding from the analyzer itself.
            (repo / "cli" / "src" / "app" / "validation" / "scripts" / "shellcheck.sh").write_text(
                "#!/bin/sh\n"
                "set -eu\n"
                '[ "$#" -eq 1 ]\n'
                '[ "${1##*/}" = "$HOOK_EXPECTED_SHELL_FILE" ]\n'
                'printf "shellcheck\\n" >> "$HOOK_INPUT_LOG"\n'
                '[ "$(cat "$1")" = valid-staged ]\n',
                encoding="utf-8",
            )
            write_executable(mock_bin / "shellcheck", "#!/bin/sh\nexit 0\n")

            log_file = fixture / "checks.log"
            env = init_fixture_repository(repo)
            env["PATH"] = f"{mock_bin}{os.pathsep}{env.get('PATH', '')}"
            env["HOOK_INPUT_LOG"] = str(log_file)

            filenames = (
                "script with spaces.sh",
                "caf\u00e9.sh",
                "tab\tname.sh",
                "line\nbreak.sh",
                'quote"name.sh',
                "-option.sh",
                "glob[name].sh",
            )
            for filename in filenames:
                if sys.platform == "win32" and any(ch in filename for ch in ('"', "\t", "\n")):
                    continue
                with self.subTest(filename=filename):
                    env["HOOK_EXPECTED_SHELL_FILE"] = filename
                    target = repo / filename
                    target.write_text("valid-staged\n", encoding="utf-8")
                    self.assertEqual(run(["git", "add", "--", filename], cwd=repo, env=env).returncode, 0)
                    target.write_text("invalid-unstaged\n", encoding="utf-8")
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8").strip(), "shellcheck")

                    target.write_text("invalid-staged\n", encoding="utf-8")
                    self.assertEqual(run(["git", "add", "--", filename], cwd=repo, env=env).returncode, 0)
                    target.write_text("valid-staged\n", encoding="utf-8")
                    res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
                    self.assertNotEqual(res.returncode, 0)
                    self.assertIn("ShellCheck reported issues", res.stdout + res.stderr)

                    self.assertEqual(run(["git", "add", "--", filename], cwd=repo, env=env).returncode, 0)
                    self.assertEqual(
                        run(
                            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                             "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture"],
                            cwd=repo, env=env,
                        ).returncode,
                        0,
                    )
                    renamed = f"renamed {filename}"
                    self.assertEqual(run(["git", "mv", "--", filename, renamed], cwd=repo, env=env).returncode, 0)
                    env["HOOK_EXPECTED_SHELL_FILE"] = renamed
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8").strip(), "shellcheck")

                    self.assertEqual(
                        run(
                            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                             "-c", "core.hooksPath=/dev/null", "commit", "-qm", "rename"],
                            cwd=repo, env=env,
                        ).returncode,
                        0,
                    )
                    self.assertEqual(run(["git", "rm", "-q", "--", renamed], cwd=repo, env=env).returncode, 0)
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8"), "")

                    self.assertEqual(
                        run(
                            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                             "-c", "core.hooksPath=/dev/null", "commit", "-qm", "deletion"],
                            cwd=repo, env=env,
                        ).returncode,
                        0,
                    )
                    res = run(["sh", "hooks/check-ci-guards.sh"], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8"), "")

    def test_staged_quoted_powershell(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            repo = fixture / "repo"
            mock_bin = fixture / "mock-bin"
            write_executable(
                mock_bin / "pwsh",
                "#!/bin/sh\n"
                "set -eu\n"
                '[ "$1" = -NoProfile ] && [ "$2" = -File ]\n'
                "shift 3\n"
                '[ "$#" -eq 1 ]\n'
                '[ "$1" = "./$HOOK_EXPECTED_FILE" ]\n'
                'printf "powershell\\n" >> "$HOOK_INPUT_LOG"\n'
                '[ "$(cat "$1")" = valid-staged ]\n',
            )
            log_file = fixture / "checks.log"
            env = init_fixture_repository(repo)
            env["PATH"] = f"{mock_bin}{os.pathsep}{env.get('PATH', '')}"
            env["HOOK_INPUT_LOG"] = str(log_file)

            filenames = (
                "script with spaces.ps1",
                "tab\tname.psm1",
                "line\nbreak.psd1",
                'quote"name.ps1',
                "-option.ps1",
                "glob[name].ps1",
            )
            check_rust = str(REPO_ROOT / "hooks" / "check-rust.sh")
            for filename in filenames:
                if sys.platform == "win32" and any(ch in filename for ch in ('"', "\t", "\n")):
                    continue
                with self.subTest(filename=filename):
                    env["HOOK_EXPECTED_FILE"] = filename
                    target = repo / filename
                    target.write_text("valid-staged\n", encoding="utf-8")
                    self.assertEqual(run(["git", "add", "--", filename], cwd=repo, env=env).returncode, 0)
                    target.write_text("invalid-unstaged\n", encoding="utf-8")
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", check_rust], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8").strip(), "powershell")

                    target.write_text("invalid-staged\n", encoding="utf-8")
                    self.assertEqual(run(["git", "add", "--", filename], cwd=repo, env=env).returncode, 0)
                    target.write_text("valid-staged\n", encoding="utf-8")
                    res = run(["sh", check_rust], cwd=repo, env=env)
                    self.assertNotEqual(res.returncode, 0)
                    self.assertIn("PSScriptAnalyzer reported issues", res.stdout + res.stderr)

                    self.assertEqual(run(["git", "add", "--", filename], cwd=repo, env=env).returncode, 0)
                    self.assertEqual(
                        run(
                            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                             "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture"],
                            cwd=repo, env=env,
                        ).returncode,
                        0,
                    )
                    renamed = f"renamed {filename}"
                    self.assertEqual(run(["git", "mv", "--", filename, renamed], cwd=repo, env=env).returncode, 0)
                    env["HOOK_EXPECTED_FILE"] = renamed
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", check_rust], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8").strip(), "powershell")

                    self.assertEqual(
                        run(
                            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                             "-c", "core.hooksPath=/dev/null", "commit", "-qm", "rename"],
                            cwd=repo, env=env,
                        ).returncode,
                        0,
                    )
                    self.assertEqual(run(["git", "rm", "-q", "--", renamed], cwd=repo, env=env).returncode, 0)
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", check_rust], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8"), "")

                    self.assertEqual(
                        run(
                            ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                             "-c", "core.hooksPath=/dev/null", "commit", "-qm", "deletion"],
                            cwd=repo, env=env,
                        ).returncode,
                        0,
                    )
                    res = run(["sh", check_rust], cwd=repo, env=env)
                    self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                    self.assertEqual(log_file.read_text(encoding="utf-8"), "")

            # Ensure argv construction preserves multiple staged inputs in one batch.
            write_executable(
                mock_bin / "pwsh",
                "#!/bin/sh\n"
                "set -eu\n"
                "shift 3\n"
                '[ "$#" -eq 2 ]\n'
                '[ "$1" = ./first.ps1 ] && [ "$2" = \'./second file.psd1\' ]\n'
                "for file do\n"
                '  [ "$(cat "$file")" = valid-staged ]\n'
                "done\n"
                'printf "powershell-batch\\n" >> "$HOOK_INPUT_LOG"\n',
            )
            (repo / "first.ps1").write_text("valid-staged\n", encoding="utf-8")
            (repo / "second file.psd1").write_text("valid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "--", "first.ps1", "second file.psd1"], cwd=repo, env=env).returncode, 0)
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_rust], cwd=repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").strip(), "powershell-batch")

            (repo / "second file.psd1").write_text("invalid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "--", "second file.psd1"], cwd=repo, env=env).returncode, 0)
            res = run(["sh", check_rust], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("PSScriptAnalyzer reported issues", res.stdout + res.stderr)

    def test_staged_ci_guard_deletions(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            mock_bin = fixture / "mock-bin"
            repo = fixture / "repo"
            for sub in ("cli", "conf", "symlinks", "docs", "hooks"):
                (repo / sub).mkdir(parents=True)

            write_executable(
                mock_bin / "cargo",
                "#!/bin/sh\n"
                "set -eu\n"
                '[ "$1" = run ]\n'
                'printf "config\\n" >> "$HOOK_INPUT_LOG"\n'
                'while [ "$#" -gt 0 ]; do\n'
                '  if [ "$1" = --root ]; then\n'
                "    shift\n"
                '    [ -f "$1/conf/required.toml" ] && [ -f "$1/symlinks/required" ]\n'
                "    exit $?\n"
                "  fi\n"
                "  shift\n"
                "done\n"
                "exit 1\n",
            )
            write_executable(
                mock_bin / "shellcheck",
                "#!/bin/sh\n"
                'echo "Deleted-only scripts must not be sent to ShellCheck" >&2\n'
                "exit 1\n",
            )

            log_file = fixture / "checks.log"
            env = init_fixture_repository(repo)
            env["PATH"] = f"{mock_bin}{os.pathsep}{env.get('PATH', '')}"
            env["HOOK_INPUT_LOG"] = str(log_file)
            env["DOTFILES_HOOKS_FULL"] = "1"

            (repo / "cli" / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.1.0"\n', encoding="utf-8")
            (repo / "conf" / "required.toml").write_text("required\n", encoding="utf-8")
            (repo / "symlinks" / "required").write_text("required\n", encoding="utf-8")
            (repo / "hooks" / "removed.sh").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "cli", "conf", "symlinks", "hooks"], cwd=repo, env=env).returncode, 0)
            self.assertEqual(
                run(
                    ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                     "-c", "core.hooksPath=/dev/null", "commit", "-qm", "baseline"],
                    cwd=repo, env=env,
                ).returncode,
                0,
            )

            check_guards = str(REPO_ROOT / "hooks" / "check-ci-guards.sh")
            for change in ("delete-config", "rename-config", "rename-source"):
                with self.subTest(change=change):
                    self.assertEqual(run(["git", "reset", "--hard", "-q", "HEAD"], cwd=repo, env=env).returncode, 0)
                    (repo / "docs").mkdir(exist_ok=True)
                    if change == "delete-config":
                        self.assertEqual(run(["git", "rm", "-q", "conf/required.toml"], cwd=repo, env=env).returncode, 0)
                        (repo / "conf").mkdir(exist_ok=True)
                        (repo / "conf" / "required.toml").write_text("required\n", encoding="utf-8")
                    elif change == "rename-config":
                        self.assertEqual(run(["git", "mv", "conf/required.toml", "docs/required.md"], cwd=repo, env=env).returncode, 0)
                    else:
                        self.assertEqual(run(["git", "mv", "symlinks/required", "docs/required.txt"], cwd=repo, env=env).returncode, 0)
                    log_file.write_text("", encoding="utf-8")
                    res = run(["sh", check_guards], cwd=repo, env=env)
                    self.assertNotEqual(res.returncode, 0, f"Full guards accepted {change}")
                    self.assertEqual(log_file.read_text(encoding="utf-8").strip(), "config")
                    self.assertIn("configuration validation failed", res.stdout + res.stderr)

            self.assertEqual(run(["git", "reset", "--hard", "-q", "HEAD"], cwd=repo, env=env).returncode, 0)
            self.assertEqual(run(["git", "rm", "-q", "hooks/removed.sh"], cwd=repo, env=env).returncode, 0)
            res = run(["sh", check_guards], cwd=repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_staged_python_and_lua_guards(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            mock_bin = fixture / "mock-bin"
            repo = fixture / "repo"
            (repo / ".github" / "linters").mkdir(parents=True)
            (repo / "scripts").mkdir(parents=True)

            write_executable(
                mock_bin / "ruff",
                "#!/bin/sh\n"
                "set -eu\n"
                '[ "$1" = check ]\n'
                "shift\n"
                "seen_config=0\n"
                'while [ "$#" -gt 0 ]; do\n'
                '  case "$1" in\n'
                "    --config) shift; [ -f \"$1\" ]; seen_config=1; shift ;;\n"
                "    --) shift; break ;;\n"
                "    *) break ;;\n"
                "  esac\n"
                "done\n"
                '[ "$seen_config" -eq 1 ]\n'
                'for file do [ "$(cat "$file")" = valid-staged ]; done\n'
                'printf "ruff\\n" >> "$HOOK_INPUT_LOG"\n',
            )
            write_executable(
                mock_bin / "luacheck",
                "#!/bin/sh\n"
                "set -eu\n"
                "seen_config=0\n"
                'while [ "$#" -gt 0 ]; do\n'
                '  case "$1" in\n'
                "    --config) shift; [ -f \"$1\" ]; seen_config=1; shift ;;\n"
                "    --) shift; break ;;\n"
                "    *) break ;;\n"
                "  esac\n"
                "done\n"
                '[ "$seen_config" -eq 1 ]\n'
                'for file do [ "$(cat "$file")" = valid-staged ]; done\n'
                'printf "luacheck\\n" >> "$HOOK_INPUT_LOG"\n',
            )

            log_file = fixture / "checks.log"
            env = init_fixture_repository(repo)
            env["PATH"] = f"{mock_bin}{os.pathsep}{env.get('PATH', '')}"
            env["HOOK_INPUT_LOG"] = str(log_file)

            (repo / ".github" / "linters" / "ruff.toml").write_text("line-length = 100\n", encoding="utf-8")
            (repo / ".github" / "linters" / "luacheckrc").write_text('std = "lua54"\n', encoding="utf-8")
            py_file = repo / "scripts" / "check tool.py"
            lua_file = repo / "-leading.lua"
            py_file.write_text("valid-staged\n", encoding="utf-8")
            lua_file.write_text("valid-staged\n", encoding="utf-8")
            self.assertEqual(
                run(["git", "add", "--", ".github/linters", "scripts/check tool.py", "-leading.lua"], cwd=repo, env=env).returncode,
                0,
            )
            py_file.write_text("invalid-unstaged\n", encoding="utf-8")
            lua_file.write_text("invalid-unstaged\n", encoding="utf-8")

            check_guards = str(REPO_ROOT / "hooks" / "check-ci-guards.sh")
            res = run(["sh", check_guards], cwd=repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["ruff", "luacheck"])

            py_file.write_text("invalid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "--", "scripts/check tool.py"], cwd=repo, env=env).returncode, 0)
            py_file.write_text("valid-staged\n", encoding="utf-8")
            res = run(["sh", check_guards], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("Ruff reported Python lint issues", res.stdout + res.stderr)
            self.assertEqual(run(["git", "add", "--", "scripts/check tool.py"], cwd=repo, env=env).returncode, 0)

            lua_file.write_text("invalid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "--", "-leading.lua"], cwd=repo, env=env).returncode, 0)
            lua_file.write_text("valid-staged\n", encoding="utf-8")
            res = run(["sh", check_guards], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("Luacheck reported Lua lint issues", res.stdout + res.stderr)
            self.assertEqual(run(["git", "add", "--", "-leading.lua"], cwd=repo, env=env).returncode, 0)

            self.assertEqual(
                run(
                    ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local",
                     "-c", "core.hooksPath=/dev/null", "commit", "-qm", "baseline"],
                    cwd=repo, env=env,
                ).returncode,
                0,
            )
            self.assertEqual(
                run(["git", "rm", "-q", "--", "scripts/check tool.py", "-leading.lua"], cwd=repo, env=env).returncode,
                0,
            )
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_guards], cwd=repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8"), "")

    def test_rust_and_shellcheck_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            mock_bin = fixture / "mock-bin"
            index_repo = fixture / "index repo"
            (index_repo / "cli" / "src").mkdir(parents=True)
            (index_repo / "conf").mkdir(parents=True)

            write_executable(
                mock_bin / "cargo",
                "#!/bin/sh\n"
                "set -eu\n"
                'printf "%s\\n" "$1" >> "$HOOK_INPUT_LOG"\n'
                'if [ -n "${EXPECT_HOOK_CARGO_ENV:-}" ]; then\n'
                '  [ -n "${DOTFILES_VERSION:-}" ]\n'
                '  case "${CARGO_TARGET_DIR:-}" in */cli/target/hooks) ;; *) exit 1 ;; esac\n'
                "fi\n"
                '[ "$(cat src/lib.rs)" = valid-staged ]\n'
                '[ "$(cat ../conf/fixture.toml)" = staged-config ]\n'
                '[ ! -e src/untracked.rs ]\n'
                'exit "${MOCK_CARGO_EXIT:-0}"\n',
            )
            write_executable(
                mock_bin / "rustc",
                "#!/bin/sh\n"
                'if [ "$1" = --version ]; then\n'
                '  printf "rustc %s (fixture)\\n" "${MOCK_RUST_VERSION:-1.95.0}"\n'
                "else\n"
                "  exit 1\n"
                "fi\n",
            )
            write_executable(
                mock_bin / "pwsh",
                "#!/bin/sh\n"
                "set -eu\n"
                '[ "$1" = -NoProfile ] && [ "$2" = -File ]\n'
                "shift 3\n"
                '[ "$#" -eq 1 ]\n'
                '[ "$(cat "$1")" = valid-staged ]\n'
                'printf "powershell\\n" >> "$HOOK_INPUT_LOG"\n',
            )
            write_executable(
                mock_bin / "shellcheck",
                "#!/bin/sh\n"
                "set -eu\n"
                "for file do\n"
                '  case "$file" in --*) continue ;; esac\n'
                '  [ -f "$file" ] || { printf "Missing ShellCheck input: %s\\n" "$file" >&2; exit 2; }\n'
                '  basename "$file" >> "$HOOK_INPUT_LOG"\n'
                "done\n"
                'exit "${SHELLCHECK_EXIT:-0}"\n',
            )

            log_file = fixture / "checks.log"
            env = init_fixture_repository(index_repo)
            env["PATH"] = f"{mock_bin}{os.pathsep}{env.get('PATH', '')}"
            env["HOOK_INPUT_LOG"] = str(log_file)

            check_rust = str(REPO_ROOT / "hooks" / "check-rust.sh")
            (index_repo / "cli" / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.1.0"\n', encoding="utf-8")
            (index_repo / "cli" / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.95.0"\n', encoding="utf-8")
            (index_repo / "cli" / "src" / "lib.rs").write_text("invalid-staged\n", encoding="utf-8")
            (index_repo / "script with spaces.ps1").write_text("valid-staged\n", encoding="utf-8")
            (index_repo / "conf" / "fixture.toml").write_text("staged-config\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "cli", "conf", "script with spaces.ps1"], cwd=index_repo, env=env).returncode, 0)
            (index_repo / "cli" / "src" / "lib.rs").write_text("valid-staged\n", encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("Commit aborted", res.stdout + res.stderr)

            self.assertEqual(run(["git", "add", "cli/src/lib.rs"], cwd=index_repo, env=env).returncode, 0)
            (index_repo / "cli" / "src" / "lib.rs").write_text("invalid-unstaged\n", encoding="utf-8")
            (index_repo / "conf" / "fixture.toml").write_text("unstaged-config\n", encoding="utf-8")
            (index_repo / "cli" / "src" / "untracked.rs").write_text("untracked\n", encoding="utf-8")
            (index_repo / "script with spaces.ps1").unlink()
            log_file.write_text("", encoding="utf-8")
            full_env = dict(env, EXPECT_HOOK_CARGO_ENV="1", DOTFILES_HOOKS_FULL="1")
            full_env.pop("CARGO_TARGET_DIR", None)
            full_env.pop("DOTFILES_VERSION", None)
            res = run(["sh", check_rust], cwd=index_repo, env=full_env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["fmt", "clippy", "test", "powershell"])

            (index_repo / "script with spaces.ps1").write_text("invalid-staged\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "script with spaces.ps1"], cwd=index_repo, env=env).returncode, 0)
            (index_repo / "script with spaces.ps1").write_text("valid-staged\n", encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("PSScriptAnalyzer reported issues", res.stdout + res.stderr)

            # Compiler pin and Cargo-only staged changes
            (index_repo / "cli" / "src" / "untracked.rs").unlink(missing_ok=True)
            (index_repo / "cli" / "src" / "lib.rs").write_text("valid-staged\n", encoding="utf-8")
            (index_repo / "script with spaces.ps1").write_text("valid-staged\n", encoding="utf-8")
            (index_repo / "conf" / "fixture.toml").write_text("staged-config\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "cli", "conf", "script with spaces.ps1"], cwd=index_repo, env=env).returncode, 0)
            self.assertEqual(
                run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local", "commit", "-qm", "baseline"],
                    cwd=index_repo, env=env).returncode,
                0,
            )

            for rel_path in (
                "cli/Cargo.toml",
                "cli/Cargo.lock",
                "cli/rust-toolchain.toml",
                "cli/.cargo/config.toml",
                ".cargo/config.toml",
            ):
                target = index_repo / rel_path
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("a", encoding="utf-8") as handle:
                    handle.write("\n# staged change\n")
                self.assertEqual(run(["git", "add", rel_path], cwd=index_repo, env=env).returncode, 0)
                log_file.write_text("", encoding="utf-8")
                res = run(["sh", check_rust], cwd=index_repo, env=env)
                self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
                self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["fmt", "clippy"], f"Failed on {rel_path}")
                self.assertEqual(
                    run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local", "commit", "-qm", "fixture"],
                        cwd=index_repo, env=env).returncode,
                    0,
                )

            self.assertEqual(run(["git", "rm", "-q", "cli/.cargo/config.toml"], cwd=index_repo, env=env).returncode, 0)
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["fmt", "clippy"])

            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=dict(env, MOCK_RUST_VERSION="1.99.0"))
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("Pinned Rust 1.95.0 required", res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8"), "")

            # A working-tree pin must not override the staged pin.
            (index_repo / "cli" / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.0"\n', encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(run(["git", "restore", "cli/rust-toolchain.toml"], cwd=index_repo, env=env).returncode, 0)
            self.assertEqual(
                run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local", "commit", "-qm", "deletion"],
                    cwd=index_repo, env=env).returncode,
                0,
            )

            self.assertEqual(run(["git", "mv", ".cargo/config.toml", ".cargo/config"], cwd=index_repo, env=env).returncode, 0)
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["fmt", "clippy"])
            self.assertEqual(
                run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local", "commit", "-qm", "rename"],
                    cwd=index_repo, env=env).returncode,
                0,
            )

            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_rust], cwd=index_repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8"), "")

            # Exercise canonical local stages using the same mock compiler and Cargo.
            (index_repo / ".github" / "workflows" / "scripts" / "linux").mkdir(parents=True)
            (index_repo / "hooks").mkdir(parents=True, exist_ok=True)
            shutil.copy2(
                REPO_ROOT / ".github" / "workflows" / "scripts" / "linux" / "check.sh",
                index_repo / ".github" / "workflows" / "scripts" / "linux" / "check.sh",
            )
            shutil.copy2(REPO_ROOT / "hooks" / "check-toolchain.sh", index_repo / "hooks" / "check-toolchain.sh")
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", ".github/workflows/scripts/linux/check.sh", "fmt", "rustdoc"], cwd=index_repo, env=env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(log_file.read_text(encoding="utf-8").splitlines(), ["fmt", "doc"])

            log_file.write_text("", encoding="utf-8")
            res = run(
                ["sh", ".github/workflows/scripts/linux/check.sh", "rustdoc"],
                cwd=index_repo,
                env=dict(env, MOCK_RUST_VERSION="1.99.0"),
            )
            self.assertNotEqual(res.returncode, 0)
            self.assertEqual(log_file.read_text(encoding="utf-8"), "")

            res = run(
                ["sh", ".github/workflows/scripts/linux/check.sh", "rustdoc"],
                cwd=index_repo,
                env=dict(env, MOCK_CARGO_EXIT="1"),
            )
            self.assertNotEqual(res.returncode, 0)

            # ShellCheck paths containing spaces and glob characters
            shell_repo = fixture / "shell repo [literal]"
            (shell_repo / "hooks").mkdir(parents=True)
            (shell_repo / "symlinks").mkdir(parents=True)
            shell_env = init_fixture_repository(shell_repo)
            shell_env["PATH"] = f"{mock_bin}{os.pathsep}{shell_env.get('PATH', '')}"
            shell_env["HOOK_INPUT_LOG"] = str(log_file)

            (shell_repo / "hooks" / "one file.sh").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            (shell_repo / "hooks" / "glob[script].sh").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            shutil.copy2(shell_repo / "hooks" / "one file.sh", shell_repo / "dotfiles.sh")
            shutil.copy2(shell_repo / "hooks" / "one file.sh", shell_repo / "symlinks" / "another file.sh")
            self.assertEqual(run(["git", "add", "hooks"], cwd=shell_repo, env=shell_env).returncode, 0)

            check_guards = str(REPO_ROOT / "hooks" / "check-ci-guards.sh")
            log_file.write_text("", encoding="utf-8")
            res = run(["sh", check_guards], cwd=shell_repo, env=shell_env)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(sorted(log_file.read_text(encoding="utf-8").splitlines()), ["glob[script].sh", "one file.sh"])

            static_analysis = str(REPO_ROOT / ".github" / "workflows" / "scripts" / "linux" / "test-static-analysis.sh")
            log_file.write_text("", encoding="utf-8")
            res = run(
                ["sh", static_analysis, "test_shellcheck"],
                cwd=shell_repo,
                env=dict(shell_env, DIR=str(shell_repo)),
            )
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertEqual(
                sorted(log_file.read_text(encoding="utf-8").splitlines()),
                ["another file.sh", "dotfiles.sh", "glob[script].sh", "one file.sh"],
            )

            res = run(["sh", check_guards], cwd=shell_repo, env=dict(shell_env, SHELLCHECK_EXIT="1"))
            self.assertNotEqual(res.returncode, 0)

            res = run(
                ["sh", static_analysis, "test_shellcheck"],
                cwd=shell_repo,
                env=dict(shell_env, DIR=str(shell_repo), SHELLCHECK_EXIT="1"),
            )
            self.assertNotEqual(res.returncode, 0)
