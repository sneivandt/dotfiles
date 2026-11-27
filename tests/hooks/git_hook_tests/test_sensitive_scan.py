"""Sensitive-pattern scanner portability, error handling, and pattern coverage tests."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import tempfile
import unittest

from .helpers import REPO_ROOT, clean_git_env, init_hook_repo, run


def _copy_sensitive_files(dest_repo: Path) -> None:
    hooks_dir = dest_repo / "hooks"
    hooks_dir.mkdir(parents=True, exist_ok=True)
    for name in ("check-sensitive.sh", "sensitive-patterns.ini", "sensitive-allowlist.ini"):
        shutil.copy2(REPO_ROOT / "hooks" / name, hooks_dir / name)


def _write_executable(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


class SensitiveScanTests(unittest.TestCase):
    def test_sensitive_scan_without_paste(self) -> None:
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(run(["git", "init", "-q", str(repo)], cwd=repo, env=env).returncode, 0)
            _copy_sensitive_files(repo)
            bin_dir = repo / "bin"
            _write_executable(bin_dir / "paste", "#!/bin/sh\nexit 127\n")
            env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

            (repo / "test-file.txt").write_text("release = v2026.07.30-1\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "test-file.txt"], cwd=repo, env=env).returncode, 0)
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertEqual(
                res.returncode,
                0,
                f"Sensitive-data scan requires unavailable paste:\n{res.stdout}{res.stderr}",
            )

    def test_sensitive_scan_unusual_paths(self) -> None:
        key_header = " ".join(("-----BEGIN", "PRIVATE", "KEY-----"))
        rsa_header = " ".join(("-----BEGIN", "RSA PRIVATE", "KEY-----"))
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(
                run(["git", "init", "-q", "--template=", str(repo)], cwd=repo, env=env).returncode,
                0,
            )
            run(["git", "config", "user.name", "Hook Test"], cwd=repo, env=env)
            run(["git", "config", "user.email", "ci@test.local"], cwd=repo, env=env)
            _copy_sensitive_files(repo)
            (repo / "baseline").write_text("baseline\n", encoding="utf-8")
            run(["git", "add", "baseline"], cwd=repo, env=env)
            run(
                ["git", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "baseline"],
                cwd=repo,
                env=env,
            )

            unusual_names = (
                "space name.txt",
                "tab\tname.txt",
                "line\nname.txt",
                r"back\slash.txt",
                "-leading-dash.txt",
            )
            for name in unusual_names:
                with self.subTest(filename=name):
                    path = repo / name
                    path.write_text(f"safe line\n{key_header}\n", encoding="utf-8")
                    self.assertEqual(
                        run(["git", "add", "--", name], cwd=repo, env=env).returncode, 0
                    )
                    res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
                    self.assertNotEqual(
                        res.returncode, 0, f"Allowed secret in unusual filename: {name!r}"
                    )
                    self.assertIn("Line 2: <redacted>", res.stdout + res.stderr)
                    run(["git", "reset", "-q", "HEAD", "--", name], cwd=repo, env=env)
                    path.unlink()

            rsa_file = repo / "rsa-key.txt"
            rsa_file.write_text(f"safe line\n{rsa_header}\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "rsa-key.txt"], cwd=repo, env=env).returncode, 0)
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            output = res.stdout + res.stderr
            self.assertEqual(
                output.count("Line 2: <redacted>"), 1, f"Duplicate RSA match:\n{output}"
            )
            run(["git", "reset", "-q", "HEAD", "--", "rsa-key.txt"], cwd=repo, env=env)
            rsa_file.unlink()

            first = repo / "first.txt"
            second = repo / "second space.txt"
            first.write_text(f"safe line\n{key_header}\n", encoding="utf-8")
            second.write_text(f"safe line\nanother safe line\n++{key_header}\n", encoding="utf-8")
            self.assertEqual(
                run(
                    ["git", "add", "--", "first.txt", "second space.txt"], cwd=repo, env=env
                ).returncode,
                0,
            )
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            output = res.stdout + res.stderr
            self.assertIn("In file: first.txt", output)
            self.assertIn("In file: second space.txt", output)
            self.assertIn("Line 2: <redacted>", output)
            self.assertIn("Line 3: <redacted>", output)
            run(
                ["git", "reset", "-q", "HEAD", "--", "first.txt", "second space.txt"],
                cwd=repo,
                env=env,
            )
            first.unlink()
            second.unlink()

            old_name = "old\tname.txt"
            new_name = "new\nname.txt"
            # Enough unchanged content to exercise Git's actual rename detection.
            lines = "".join(f"safe line {n}\n" for n in range(1, 31))
            (repo / old_name).write_text(lines, encoding="utf-8")
            self.assertEqual(run(["git", "add", "--", old_name], cwd=repo, env=env).returncode, 0)
            self.assertEqual(
                run(
                    ["git", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "rename baseline"],
                    cwd=repo,
                    env=env,
                ).returncode,
                0,
            )
            (repo / old_name).rename(repo / new_name)
            with (repo / new_name).open("a", encoding="utf-8") as handle:
                handle.write(f"{key_header}\n")
            self.assertEqual(
                run(["git", "add", "-A", "--", old_name, new_name], cwd=repo, env=env).returncode, 0
            )
            summary = run(["git", "diff", "--cached", "--summary"], cwd=repo, env=env).stdout
            self.assertIn("rename", summary, "Unusual-path fixture did not produce a rename")
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0, "Allowed new content in a renamed unusual path")
            self.assertIn("Line 31: <redacted>", res.stdout + res.stderr)

            self.assertEqual(
                run(
                    ["git", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "deletion baseline"],
                    cwd=repo,
                    env=env,
                ).returncode,
                0,
            )
            (repo / new_name).unlink()
            self.assertEqual(
                run(["git", "add", "-A", "--", new_name], cwd=repo, env=env).returncode, 0
            )
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertEqual(
                res.returncode, 0, f"Sensitive scan blocked a deletion:\n{res.stdout}{res.stderr}"
            )

    def test_sensitive_scan_errors(self) -> None:
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(
                run(["git", "init", "-q", "--template=", str(repo)], cwd=repo, env=env).returncode,
                0,
            )
            (repo / "hooks").mkdir()
            shutil.copy2(
                REPO_ROOT / "hooks" / "check-sensitive.sh", repo / "hooks" / "check-sensitive.sh"
            )

            (repo / "hooks" / "sensitive-patterns.ini").write_text(
                "(unterminated\n", encoding="utf-8"
            )
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("Cannot validate", res.stdout + res.stderr)

            (repo / "hooks" / "sensitive-patterns.ini").write_text(
                "sensitive-marker\n", encoding="utf-8"
            )
            (repo / "hooks" / "sensitive-allowlist.ini").write_text(
                "(unterminated\n", encoding="utf-8"
            )
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0)
            (repo / "hooks" / "sensitive-allowlist.ini").unlink()

            (repo / "test-file.txt").write_text("safe content\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "test-file.txt"], cwd=repo, env=env).returncode, 0)
            bin_dir = repo / "bin"
            _write_executable(
                bin_dir / "grep",
                "#!/bin/sh\n"
                "for argument do\n"
                '  if [ "$argument" = /dev/null ]; then exit 1; fi\n'
                "done\n"
                "exit 2\n",
            )
            grep_env = dict(env, PATH=f"{bin_dir}{os.pathsep}{env.get('PATH', '')}")
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=grep_env)
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("scan failed", res.stdout + res.stderr)
            (bin_dir / "grep").unlink()

            _write_executable(
                bin_dir / "git",
                "#!/bin/sh\n"
                'case "$1" in\n'
                "  rev-parse) exit 1 ;;\n"
                "  hash-object) printf 'unused-tree\\n' ;;\n"
                "  diff) exit 2 ;;\n"
                "  *) exit 2 ;;\n"
                "esac\n",
            )
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=grep_env)
            self.assertNotEqual(res.returncode, 0)

    def test_sensitive_scan_reads_staged_patterns_and_allowlist(self) -> None:
        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = clean_git_env(repo)
            self.assertEqual(
                run(["git", "init", "-q", "--template=", str(repo)], cwd=repo, env=env).returncode,
                0,
            )
            (repo / "hooks").mkdir()
            shutil.copy2(
                REPO_ROOT / "hooks" / "check-sensitive.sh", repo / "hooks" / "check-sensitive.sh"
            )
            patterns = repo / "hooks" / "sensitive-patterns.ini"
            allowlist = repo / "hooks" / "sensitive-allowlist.ini"
            patterns.write_text("STAGED_ONLY_SECRET\n", encoding="utf-8")
            allowlist.write_text("ALLOW_STAGED_MARKER\n", encoding="utf-8")
            self.assertEqual(
                run(
                    ["git", "add", "hooks/sensitive-patterns.ini", "hooks/sensitive-allowlist.ini"],
                    cwd=repo,
                    env=env,
                ).returncode,
                0,
            )
            self.assertEqual(
                run(
                    [
                        "git",
                        "-c",
                        "user.name=Fixture",
                        "-c",
                        "user.email=fixture@test.local",
                        "-c",
                        "core.hooksPath=/dev/null",
                        "commit",
                        "-qm",
                        "baseline",
                    ],
                    cwd=repo,
                    env=env,
                ).returncode,
                0,
            )

            # Unstaged worktree edits to patterns/allowlist must not affect the staged scan.
            patterns.write_text("WORKTREE_ONLY_SECRET\n", encoding="utf-8")
            allowlist.write_text("STAGED_ONLY_SECRET\n", encoding="utf-8")

            sample = repo / "note.txt"
            sample.write_text("STAGED_ONLY_SECRET\n", encoding="utf-8")
            self.assertEqual(run(["git", "add", "note.txt"], cwd=repo, env=env).returncode, 0)
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0, "Worktree allowlist leaked into staged scan")

            # Staged allowlist update takes effect even when worktree drops it.
            allowlist.write_text("STAGED_ONLY_SECRET\n", encoding="utf-8")
            self.assertEqual(
                run(["git", "add", "hooks/sensitive-allowlist.ini"], cwd=repo, env=env).returncode,
                0,
            )
            allowlist.write_text("UNRELATED\n", encoding="utf-8")
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertEqual(
                res.returncode, 0, f"Staged allowlist was ignored:\n{res.stdout}{res.stderr}"
            )

            # Staging removal of the allowlist must not fall back to the worktree copy.
            self.assertEqual(
                run(
                    ["git", "rm", "-fq", "hooks/sensitive-allowlist.ini"], cwd=repo, env=env
                ).returncode,
                0,
            )
            allowlist.write_text("STAGED_ONLY_SECRET\n", encoding="utf-8")
            res = run(["sh", "hooks/check-sensitive.sh"], cwd=repo, env=env)
            self.assertNotEqual(res.returncode, 0, "Deleted allowlist fell back to worktree file")

    def test_sensitive_patterns_and_clean_commits(self) -> None:
        # Construct synthetic test fixtures dynamically so this file stays
        # clean under the repository's pre-commit sensitive-content scanner.
        sample_aws = "".join(("AKIAIOSFODNN", "7EXAMPLE"))
        sample_ghp = "".join(("ghp_", "12345678901234567890", "12345678901234567890"))
        sample_gho = "".join(("gho_", "abcdefghijklmnopqrstuvwx", "yz12345678901234"))
        sample_pat = "".join(("github_pat_", "abcdefghijklmnopqrstuv"))
        sample_hex = "".join(("1234567890abcdef", "1234567890abcdef"))
        sample_secret = "".join(("abcdef1234567890", "abcdef1234567890"))
        sample_pass = "".join(("MySecret", "Pass123"))
        sample_passwd = "".join(("Another", "Password456"))
        sample_jwt = ".".join(
            (
                "eyJhbGciOiJIUzI1NiJ9",
                "eyJzdWIiOiJ0ZXN0In0",
                "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            )
        )
        sample_val = "".join(("abc123+def456/", "ghi789==abcd"))
        sample_rsa = " ".join(("-----BEGIN", "RSA PRIVATE", "KEY-----"))
        sample_ssh = " ".join(("-----BEGIN", "OPENSSH PRIVATE", "KEY-----"))
        sample_mysql = "".join(("mysql", "://", "user", ":", "password", "@localhost:3306/db"))
        sample_pg = "".join(("postgresql", "://", "admin", ":", "secret", "@db.example.com/mydb"))
        sample_heroku = "-".join(("550e8400", "e29b", "41d4", "a716", "446655440000"))
        sample_slack = "-".join(
            ("xoxb", "1111111111111", "2222222222222", "EXAMPLEEXAMPLEEXAMPLEEXA")
        )
        sample_stripe = "_".join(("sk", "test", "EXAMPLEKEY1234567890abcd"))
        sample_google = "".join(("AIzaSyC1234567890", "abcdefghijklmnopqrstuvw"))
        sample_gitlab = "-".join(("glpat", "abcdefghijklmnopqrstuvwxyz"))
        sample_generic = "".join(("a1b2c3d4e5f6g7h8i9j0", "k1l2m3n4o5p6q7r8s9t0"))
        sample_uuid_a = "-".join(("12345678", "1234", "1234", "1234", "123456789abc"))
        sample_uuid_b = "-".join(("abcdef12", "3456", "7890", "abcd", "ef1234567890"))
        sample_uuid_c = "-".join(("11111111", "2222", "3333", "4444", "555555555555"))
        sample_email = "@".join(("john.doe", "example.com"))
        sample_phone_fmt = "".join(("(", "555", ") ", "123", "-", "4567"))
        sample_phone_dash = "-".join(("555", "123", "4567"))
        sample_phone_plain = "".join(("555", "123", "4567"))
        sample_phone_cc = "".join(("+1-", "555", "-", "123", "-", "4567"))
        sample_ssn = "-".join(("123", "45", "6789"))
        sample_ssn_plain = "".join(("123", "45", "6789"))
        sample_cc_spaces = " ".join(("4532", "1234", "5678", "9010"))
        sample_cc_dashes = "-".join(("4532", "1234", "5678", "9010"))
        sample_ipv4 = ".".join(("192", "168", "1", "100"))
        sample_ipv6 = ":".join(("2001", "0db8", "85a3", "0000", "0000", "8a2e", "0370", "7334"))

        blocked_cases = (
            ("AWS access key", f"{'aws_access'}_{'key_id'} = {sample_aws}"),
            ("AWS key ID format", f"export AWS_KEY={sample_aws}"),
            ("GitHub PAT", f"token = {sample_ghp}"),
            ("GitHub token assignment", f"GITHUB_TOKEN={sample_gho}"),
            ("GitHub fine-grained PAT", f"GITHUB_TOKEN={sample_pat}"),
            ("API key", f'{"api"}{"key"} = "{sample_hex}"'),
            ("API key (single-quoted)", f"{'api'}{'key'} = '{sample_hex}'"),
            ("Secret key", f'{"secret"}_{"key"}: "{sample_secret}"'),
            ("Password", f'{"pass"}{"word"} = "{sample_pass}"'),
            ("Password (single-quoted)", f"{'pass'}{'word'} = '{sample_pass}'"),
            ("Passwd", f'{"pass"}{"wd"}: "{sample_passwd}"'),
            ("Authorization Bearer JWT", f"Authorization: Bearer {sample_jwt}"),
            ("Quoted bearer assignment", f'{"bear"}{"er"} = "{sample_val}"'),
            ("Quoted authorization Bearer", f'{"author"}{"ization"}: "Bearer {sample_val}"'),
            ("RSA private key", sample_rsa),
            ("OpenSSH private key", sample_ssh),
            ("MySQL connection", sample_mysql),
            ("PostgreSQL connection", sample_pg),
            ("Heroku API key", f"HEROKU_API_KEY={sample_heroku}"),
            ("Slack bot token", f"SLACK_TOKEN={sample_slack}"),
            ("Stripe secret key", f"STRIPE_SECRET={sample_stripe}"),
            ("Google API key", f"GOOGLE_API_KEY={sample_google}"),
            ("GitLab PAT", f"GITLAB_TOKEN={sample_gitlab}"),
            ("OAuth client secret", f'{"client"}_{"secret"} = "{sample_hex}"'),
            ("Generic secret", f'{"sec"}{"ret"} = "{sample_generic}"'),
            ("UUID in URL", f"https://example.com/auth/{sample_uuid_a}"),
            ("GUID assignment", f"tenant_id = {sample_uuid_b}"),
            ("UUID constant", f"const CLIENT_ID = '{sample_uuid_c}'"),
            ("Email address", f"user_email = {sample_email}"),
            ("Phone number (formatted)", f"phone = {sample_phone_fmt}"),
            ("Phone number (dashes)", f"contact = {sample_phone_dash}"),
            ("Phone number (plain)", f"phone_number = {sample_phone_plain}"),
            ("Phone number (+1 country code)", f"phone = {sample_phone_cc}"),
            ("SSN (formatted)", f"ssn = {sample_ssn}"),
            ("SSN (plain)", f"social_security_number = {sample_ssn_plain}"),
            ("Credit card (spaces)", f"cc = {sample_cc_spaces}"),
            ("Credit card (dashes)", f"card_number = {sample_cc_dashes}"),
            ("IPv4 address", f"server_ip = {sample_ipv4}"),
            ("IPv6 address", f"ipv6 = {sample_ipv6}"),
            (
                "Commit SHA alongside real PII",
                f'"commit": "64a654ef4a20db1727938338310209b6a63f60c9", "ssn": "{sample_ssn}"',
            ),
        )

        clean_cases = (
            "# This is a clean comment",
            "const API_URL = 'https://api.example.com'",
            "password_field = 'password'  # Field name, not actual password",
            "const SECRET_LENGTH = 32  # Configuration constant",
            '"commit": "64a654ef4a20db1727938338310209b6a63f60c9"',
        )

        with tempfile.TemporaryDirectory() as repo_str:
            repo = Path(repo_str)
            env = init_hook_repo(repo)
            test_file = repo / "test-file.txt"

            for pattern_type, content in blocked_cases:
                with self.subTest(blocked=pattern_type):
                    test_file.write_text(f"{content}\n", encoding="utf-8")
                    self.assertEqual(
                        run(["git", "add", "test-file.txt"], cwd=repo, env=env).returncode, 0
                    )
                    commit = run(
                        ["git", "commit", "-m", f"Test commit with {pattern_type}"],
                        cwd=repo,
                        env=env,
                    )
                    output = commit.stdout + commit.stderr
                    try:
                        self.assertNotEqual(
                            commit.returncode, 0, f"Hook allowed commit with {pattern_type}"
                        )
                        self.assertIn("Potential sensitive information detected", output)
                        self.assertNotIn(
                            content, output, f"Hook printed detected {pattern_type} content"
                        )
                    finally:
                        run(["git", "reset", "-q", "HEAD", "test-file.txt"], cwd=repo, env=env)
                        test_file.unlink(missing_ok=True)

            # Staged file deleted from the worktree must still be scanned.
            staged_only = repo / "staged-only-secret.txt"
            staged_value = "".join(("abcdef12345", "67890abcdef", "12345", "67890"))
            staged_only.write_text(f'{"secret"}_{"key"}: "{staged_value}"\n', encoding="utf-8")
            self.assertEqual(
                run(["git", "add", "staged-only-secret.txt"], cwd=repo, env=env).returncode, 0
            )
            staged_only.unlink()
            commit = run(
                ["git", "commit", "-m", "Test staged-only sensitive file"], cwd=repo, env=env
            )
            try:
                self.assertNotEqual(commit.returncode, 0)
                self.assertIn(
                    "Potential sensitive information detected", commit.stdout + commit.stderr
                )
            finally:
                run(["git", "reset", "-q", "HEAD", "staged-only-secret.txt"], cwd=repo, env=env)

            for content in clean_cases:
                with self.subTest(clean=content):
                    test_file.write_text(f"{content}\n", encoding="utf-8")
                    self.assertEqual(
                        run(["git", "add", "test-file.txt"], cwd=repo, env=env).returncode, 0
                    )
                    commit = run(["git", "commit", "-m", "Test clean commit"], cwd=repo, env=env)
                    try:
                        self.assertEqual(
                            commit.returncode,
                            0,
                            f"Hook blocked clean commit:\n{commit.stdout}{commit.stderr}",
                        )
                    finally:
                        if commit.returncode == 0:
                            run(["git", "reset", "--soft", "HEAD~1"], cwd=repo, env=env)
                        run(["git", "reset", "-q", "HEAD", "test-file.txt"], cwd=repo, env=env)
                        test_file.unlink(missing_ok=True)
