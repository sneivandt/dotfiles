"""Allowlist and portable POSIX regex tests for hooks/check-sensitive.sh."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import tempfile
import unittest

from .helpers import REPO_ROOT, init_fixture_repository, run, write_executable


def _copy_sensitive_hooks(dest_repo: Path) -> None:
    hooks_dir = dest_repo / "hooks"
    hooks_dir.mkdir(parents=True, exist_ok=True)
    for name in ("check-sensitive.sh", "sensitive-patterns.ini", "sensitive-allowlist.ini"):
        shutil.copy2(REPO_ROOT / "hooks" / name, hooks_dir / name)


def _synthetic_ghp() -> str:
    return "".join(("ghp_", "abcdefghijklmnopqrstuvwxyz", "abcdefghijklmn"))


class AllowlistTests(unittest.TestCase):
    def test_github_ssh_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            env = init_fixture_repository(fixture)
            _copy_sensitive_hooks(fixture)
            token = _synthetic_ghp()

            remotes = (
                "git@github.com:fixture/repo.git",
                "ssh://git@github.com/fixture/repo.git",
                "git@ssh.dev.azure.com:v3/fixture/project/repo",
                "ssh://git@ssh.dev.azure.com/v3/fixture/project/repo",
            )
            for remote in remotes:
                with self.subTest(remote=remote):
                    (fixture / "remote.txt").write_text(f"{remote}\n", encoding="utf-8")
                    self.assertEqual(
                        run(["git", "add", "remote.txt"], cwd=fixture, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
                    self.assertEqual(
                        res.returncode,
                        0,
                        f"Public Git SSH URL was rejected: {remote}\n{res.stdout}{res.stderr}",
                    )

                    (fixture / "remote.txt").write_text(
                        f"{remote} token = {token}\n", encoding="utf-8"
                    )
                    self.assertEqual(
                        run(["git", "add", "remote.txt"], cwd=fixture, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
                    self.assertNotEqual(
                        res.returncode,
                        0,
                        f"Git SSH exception hid a token on the same line: {remote}",
                    )

            for lookalike in ("github.com.example", "ssh.dev.azure.com.example"):
                with self.subTest(lookalike=lookalike):
                    (fixture / "remote.txt").write_text(f"git@{lookalike}\n", encoding="utf-8")
                    self.assertEqual(
                        run(["git", "add", "remote.txt"], cwd=fixture, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
                    self.assertNotEqual(
                        res.returncode,
                        0,
                        f"Git SSH exception accepted a lookalike email domain: {lookalike}",
                    )

    def test_shell_sha256_case_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            env = init_fixture_repository(fixture)
            _copy_sensitive_hooks(fixture)
            checksum = "0" * 64
            token = _synthetic_ghp()

            (fixture / "shim.sh").write_text(
                f"  {checksum} | \\\n  {checksum})\n", encoding="utf-8"
            )
            self.assertEqual(run(["git", "add", "shim.sh"], cwd=fixture, env=env).returncode, 0)
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
            self.assertEqual(
                res.returncode,
                0,
                f"Shell SHA-256 case digest was rejected:\n{res.stdout}{res.stderr}",
            )

            (fixture / "shim.sh").write_text(f"  {checksum}) token = {token}\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "shim.sh"], cwd=fixture, env=env).returncode, 0)
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
            self.assertNotEqual(
                res.returncode,
                0,
                "Shell SHA-256 case digest exception hid a token on the same line",
            )

    def test_pkgbuild_checksum_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            env = init_fixture_repository(fixture)
            _copy_sensitive_hooks(fixture)
            checksum = "0" * 64
            token = _synthetic_ghp()

            for variable in ("sha256sums", "sha256sums_x86_64"):
                with self.subTest(variable=variable):
                    (fixture / "PKGBUILD").write_text(
                        f"{variable}=('{checksum}' '{checksum}')\n", encoding="utf-8"
                    )
                    self.assertEqual(
                        run(["git", "add", "PKGBUILD"], cwd=fixture, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
                    self.assertEqual(
                        res.returncode,
                        0,
                        f"PKGBUILD checksum was rejected:\n{res.stdout}{res.stderr}",
                    )

                    (fixture / "PKGBUILD").write_text(
                        f"{variable}=('{checksum}'); token = {token}\n", encoding="utf-8"
                    )
                    self.assertEqual(
                        run(["git", "add", "PKGBUILD"], cwd=fixture, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=fixture, env=env)
                    self.assertNotEqual(
                        res.returncode,
                        0,
                        "PKGBUILD checksum exception hid a token on the same line",
                    )

    def test_sensitive_posix_patterns(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_str:
            fixture = Path(fixture_str)
            repo = fixture / "repo"
            bin_dir = fixture / "bin"
            env = init_fixture_repository(repo)
            _copy_sensitive_hooks(repo)

            real_grep = shutil.which("grep")
            real_sed = shutil.which("sed")
            self.assertIsNotNone(real_grep)
            self.assertIsNotNone(real_sed)

            write_executable(
                bin_dir / "grep",
                "#!/bin/sh\n"
                "for arg do\n"
                "  case \"$arg\" in *'\\s'*|*'\\b'*) exit 2 ;; esac\n"
                "done\n"
                'exec "$STRICT_GREP" "$@"\n',
            )
            write_executable(
                bin_dir / "sed",
                "#!/bin/sh\n"
                "for arg do\n"
                '  case "$arg" in *gI*) exit 2 ;; esac\n'
                "done\n"
                'exec "$STRICT_SED" "$@"\n',
            )
            env["STRICT_GREP"] = real_grep
            env["STRICT_SED"] = real_sed
            env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

            zero_sha = "0" * 40
            (repo / "fixture.txt").write_text(f"uses: OWNER/REPO@{zero_sha}\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "fixture.txt"], cwd=repo, env=env).returncode, 0)
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertEqual(
                res.returncode,
                0,
                f"Portable scanner rejected mixed-case allowlisted span:\n{res.stdout}{res.stderr}",
            )

            for field, length in (("client_secret", 24), ("key", 40)):
                with self.subTest(field=field):
                    value = "x" * length
                    (repo / "fixture.txt").write_text(
                        f'uses: OWNER/REPO@{zero_sha}; {field} \t= \t"{value}"\n',
                        encoding="utf-8",
                    )
                    self.assertEqual(
                        run(["git", "add", "fixture.txt"], cwd=repo, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
                    self.assertNotEqual(
                        res.returncode,
                        0,
                        f"Portable {field} rule missed a secret beside an allowlisted span",
                    )
                    self.assertIn("Potential sensitive information", res.stdout + res.stderr)
