"""Bootstrap download, checksum, recovery, release-repository, and API tests for dotfiles.sh."""

from __future__ import annotations

import fcntl
import os
from pathlib import Path
import re
import tempfile
import unittest

from .helpers import WRAPPER_SRC, copy_wrapper, run, symlink_system_tools, write_executable

_SYSTEM_TOOLS = (
    "awk",
    "cat",
    "chmod",
    "dirname",
    "flock",
    "mkdir",
    "mktemp",
    "mv",
    "readlink",
    "rm",
    "rmdir",
    "sha256sum",
    "uname",
    "grep",
)

_FAKE_BOOTSTRAP_DOWNLOADER = """#!/bin/sh
set -o errexit
set -o nounset

out=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o|-qO)
      shift
      out="$1"
      ;;
    http*)
      url="$1"
      ;;
  esac
  shift
done

case "$url" in
  */releases/latest)
    if [ "${WRAPPER_RELEASE_JSON+x}" = x ]; then
      printf '%s\\n' "$WRAPPER_RELEASE_JSON"
    else
      printf '{"tag_name":"v9.9.9"}\\n'
    fi
    exit "${WRAPPER_RELEASE_STATUS:-0}"
    ;;
  */releases/download/v9.9.9/checksums.sha256)
    cached="$DOTFILES_ROOT/bin/dotfiles"
    if [ "${WRAPPER_EMPTY_CACHE:-0}" = 1 ]; then
      [ -f "$cached" ] && [ ! -s "$cached" ]
    else
      [ ! -e "$cached" ]
    fi || {
      echo "bootstrap published its binary before verification" >&2
      exit 1
    }
    sum=$(sha256sum < "$(cat "$DOTFILES_ROOT/download-path")" | awk '{print $1}')
    printf '%s  dotfiles-linux-x86_64\\n' "$sum" > "$out"
    ;;
  */releases/download/v9.9.9/dotfiles-linux-x86_64)
    mkdir -p "$(dirname "$out")"
    printf '%s\\n' "$out" > "$DOTFILES_ROOT/download-path"
    cat > "$out" <<'BIN'
#!/bin/sh
if [ "$1" = --version ]; then
  printf 'dotfiles v9.9.9\\n'
  exit 0
fi
printf '%s\\n' "$DOTFILES_ROOT" > "$DOTFILES_ROOT/root.txt"
printf '%s\\n' "$DOTFILES_WRAPPER" > "$DOTFILES_ROOT/wrapper.txt"
printf '%s\\n' "$@" > "$DOTFILES_ROOT/args.txt"
printf '%s\\n' '{"type":"fixture"}'
BIN
    ;;
  *)
    echo "unexpected URL: $url" >&2
    exit 1
    ;;
esac
"""


class WrapperBootstrapTests(unittest.TestCase):
    def _run_bootstrap_scenario(
        self,
        *,
        suffix: str = "",
        empty_cache: bool = False,
        block_publish: bool = False,
        downloader: str = "curl",
        release_json: str | None = None,
        release_status: int | None = None,
        expect_release_error: bool = False,
    ) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-bootstrap-") as base_str:
            tmpdir = Path(base_str).resolve() / f"root{suffix}"
            wrapper = copy_wrapper(tmpdir)
            fake_bin = tmpdir / "fake-bin"
            write_executable(fake_bin / downloader, _FAKE_BOOTSTRAP_DOWNLOADER)

            env = dict(
                os.environ,
                DOTFILES_RELEASE_REPO="fixture/releases",
                DOTFILES_SKIP_ATTESTATION="1",
            )
            for key in (
                "WRAPPER_EMPTY_CACHE",
                "WRAPPER_RELEASE_JSON",
                "WRAPPER_RELEASE_STATUS",
            ):
                env.pop(key, None)

            if downloader == "wget":
                symlink_system_tools(fake_bin, _SYSTEM_TOOLS)
                env["PATH"] = str(fake_bin)
            else:
                env["PATH"] = f"{fake_bin}:{os.environ.get('PATH', '')}"

            if empty_cache:
                env["WRAPPER_EMPTY_CACHE"] = "1"
                cached = tmpdir / "bin" / "dotfiles"
                cached.parent.mkdir(parents=True, exist_ok=True)
                cached.write_text("", encoding="utf-8")
                cached.chmod(0o755)
            if release_json is not None:
                env["WRAPPER_RELEASE_JSON"] = release_json
            if release_status is not None:
                env["WRAPPER_RELEASE_STATUS"] = str(release_status)

            if block_publish:
                lock_path = tmpdir / "bin" / ".dotfiles.update-state"
                lock_path.parent.mkdir(parents=True, exist_ok=True)
                with lock_path.open("a+b") as lock_file:
                    fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                    res = run([str(wrapper), "update", "-p", "desktop", "-n"], cwd=tmpdir, env=env)
                self.assertEqual(
                    res.returncode, 1, "Bootstrap published while update lock was held"
                )
                self.assertIn("CLI update in progress", res.stderr)
                self.assertFalse((tmpdir / "bin" / "dotfiles").exists())
                return

            res = run([str(wrapper), "update", "-p", "desktop", "-n"], cwd=tmpdir, env=env)
            if expect_release_error:
                self.assertNotEqual(
                    res.returncode, 0, "Wrapper accepted unsuccessful release lookup"
                )
                self.assertFalse((tmpdir / "download-path").exists())
                self.assertFalse((tmpdir / "args.txt").exists())
                self.assertIn("Failed to resolve latest release tag", res.stderr)
                return

            self.assertEqual(res.returncode, 0, f"Bootstrap failed:\n{res.stderr}")
            self.assertEqual(res.stdout.strip(), '{"type":"fixture"}')
            self.assertRegex(res.stderr, r"(?m)^Bootstrap ·")
            self.assertRegex(res.stderr, r"(?m)^Downloaded ·")
            self.assertEqual(
                (tmpdir / "args.txt").read_text(encoding="utf-8").splitlines(),
                ["update", "-p", "desktop", "-n"],
            )
            self.assertEqual((tmpdir / "root.txt").read_text(encoding="utf-8").strip(), str(tmpdir))
            self.assertEqual((tmpdir / "wrapper.txt").read_text(encoding="utf-8").strip(), "sh")
            self.assertTrue(os.access(tmpdir / "bin" / "dotfiles", os.X_OK))

    def test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args(self) -> None:
        scenarios = (
            ("default", {}),
            ("backslash_path", {"suffix": " path with \\ backslash"}),
            ("empty_cache", {"empty_cache": True}),
            ("blocked_publish", {"block_publish": True}),
        )
        for label, kwargs in scenarios:
            with self.subTest(scenario=label):
                self._run_bootstrap_scenario(**kwargs)

    def test_wrapper_recovers_truncated_cli(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-recovery-") as tmp_str:
            fixture = Path(tmp_str)
            wrapper = copy_wrapper(fixture)
            bin_dir = fixture / "bin"
            bin_dir.mkdir()
            installed = bin_dir / "dotfiles"
            backup = bin_dir / ".dotfiles.old"
            journal = bin_dir / ".dotfiles.update-state"

            write_executable(installed, "")
            write_executable(backup, "")
            journal.write_text("x", encoding="utf-8")

            res = run([str(wrapper), "--version"], cwd=fixture)
            self.assertNotEqual(res.returncode, 0, "Recovery accepted an empty executable")
            self.assertGreater(journal.stat().st_size, 0)
            self.assertEqual(installed.stat().st_size, 0)
            self.assertEqual(backup.stat().st_size, 0)
            self.assertIn("Neither the installed CLI nor its backup starts", res.stderr)

            write_executable(backup, '#!/bin/sh\nprintf "dotfiles v9.9.9\\n"\n')
            res = run([str(wrapper), "--version"], cwd=fixture)
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertEqual(res.stdout.strip(), "dotfiles v9.9.9")
            self.assertFalse(backup.exists())
            self.assertEqual(journal.stat().st_size, 0)
            self.assertRegex(res.stderr, r"(?m)^Restored previous CLI")

            installed.unlink()
            journal.unlink()
            write_executable(backup, "")
            res = run([str(wrapper), "--version"], cwd=fixture)
            self.assertNotEqual(res.returncode, 0, "Legacy recovery accepted an empty backup")
            self.assertTrue(backup.is_file())
            self.assertFalse(installed.exists())

            healthy = (
                "#!/bin/sh\n"
                'if [ -n "${DOTFILES_COMPLETE:-}" ]; then\n'
                "  printf 'complete:%s\\n' \"$DOTFILES_COMPLETE\"\n"
                "  exit 0\n"
                "fi\n"
                "[ \"${1:-}\" = --version ] && printf 'dotfiles v9.9.9\\n'\n"
            )
            write_executable(installed, healthy)
            write_executable(backup, healthy)
            journal.write_text("x", encoding="utf-8")
            env = dict(os.environ, DOTFILES_COMPLETE="zsh")
            res = run([str(wrapper)], cwd=fixture, env=env)
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertEqual(res.stdout.strip(), "complete:zsh")
            self.assertEqual(journal.stat().st_size, 0)
            self.assertTrue(backup.is_file())

    def test_wrapper_release_metadata_formats(self) -> None:
        for downloader in ("curl", "wget"):
            for label, kwargs in (
                (
                    "tag_in_middle",
                    {
                        "release_json": '{"url":"https://api.github.com/example","tag_name":"v9.9.9","name":"release"}'
                    },
                ),
                (
                    "multiline_spaced",
                    {
                        "release_json": '{\n    "url": "https://api.github.com/example",\n    "name": "release", "tag_name" : "v9.9.9"\n  }'
                    },
                ),
                (
                    "nonzero_status",
                    {"release_status": 23, "expect_release_error": True},
                ),
                (
                    "missing_tag",
                    {
                        "release_json": '{"name":"release without a tag"}',
                        "expect_release_error": True,
                    },
                ),
            ):
                with self.subTest(downloader=downloader, case=label):
                    self._run_bootstrap_scenario(downloader=downloader, **kwargs)

    def test_wrapper_chmod_after_checksum(self) -> None:
        lines = WRAPPER_SRC.read_text(encoding="utf-8").splitlines()
        verify_line = next(
            (i for i, line in enumerate(lines, start=1) if '_verify_checksum "' in line),
            None,
        )
        chmod_line = next(
            (i for i, line in enumerate(lines, start=1) if "chmod +x" in line),
            None,
        )
        self.assertIsNotNone(verify_line, f"Missing _verify_checksum call in {WRAPPER_SRC}")
        self.assertIsNotNone(chmod_line, f"Missing chmod +x in {WRAPPER_SRC}")
        self.assertGreater(
            chmod_line,
            verify_line,
            f"chmod +x (line {chmod_line}) must appear after _verify_checksum (line {verify_line})",
        )

    def test_wrapper_failed_bootstrap_preserves_cached_binary(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-staging-") as tmp_str:
            fixture = Path(tmp_str).resolve()
            wrapper = copy_wrapper(fixture)
            fake_bin = fixture / "fake-bin"
            write_executable(
                fake_bin / "curl",
                "#!/bin/sh\n"
                "set -eu\n"
                'out=""\n'
                'url=""\n'
                'while [ "$#" -gt 0 ]; do\n'
                '  case "$1" in\n'
                "    -o) shift; out=$1 ;;\n"
                "    http*) url=$1 ;;\n"
                "  esac\n"
                "  shift\n"
                "done\n"
                'case "$url" in\n'
                '  */releases/latest) printf \'{"tag_name":"v9.9.9"}\\n\' ;;\n'
                "  */checksums.sha256)\n"
                '    if [ "$WRAPPER_FAILURE" = checksum ]; then\n'
                "      printf 'invalid  dotfiles-linux-x86_64\\n' > \"$out\"\n"
                "    else\n"
                '      sum=$(sha256sum "$(cat "$DOTFILES_ROOT/download-path")" | awk \'{print $1}\')\n'
                '      printf \'%s  dotfiles-linux-x86_64\\n\' "$sum" > "$out"\n'
                "    fi\n"
                "    ;;\n"
                "  */dotfiles-linux-x86_64)\n"
                '    printf \'#!/bin/sh\\nprintf "dotfiles v9.9.9\\\\n"\\n\' > "$out"\n'
                '    printf \'%s\\n\' "$out" > "$DOTFILES_ROOT/download-path"\n'
                "    printf '#!/bin/sh\\nexit 0\\n' > \"$DOTFILES_ROOT/bin/dotfiles\"\n"
                '    chmod +x "$DOTFILES_ROOT/bin/dotfiles"\n'
                '    [ "$WRAPPER_FAILURE" != download ]\n'
                "    ;;\n"
                "  *) exit 1 ;;\n"
                "esac\n",
            )
            write_executable(fake_bin / "gh", '#!/bin/sh\n[ "$WRAPPER_FAILURE" = publication ]\n')
            write_executable(fake_bin / "uname", '#!/bin/sh\nprintf "x86_64\\n"\n')

            expected = "#!/bin/sh\nexit 0\n"
            base_env = dict(
                os.environ,
                PATH=f"{fake_bin}:{os.environ.get('PATH', '')}",
                DOTFILES_RELEASE_REPO="fixture/releases",
                DOTFILES_SKIP_ATTESTATION="0",
            )
            for failure in ("download", "checksum", "attestation", "publication"):
                with self.subTest(failure=failure):
                    env = dict(base_env, WRAPPER_FAILURE=failure)
                    res = run([str(wrapper), "--version"], cwd=fixture, env=env)
                    self.assertNotEqual(res.returncode, 0, f"{failure} failure was accepted")
                    cached = fixture / "bin" / "dotfiles"
                    self.assertTrue(os.access(cached, os.X_OK))
                    self.assertEqual(cached.read_text(encoding="utf-8"), expected)
                    entries = [
                        p.name
                        for p in (fixture / "bin").iterdir()
                        if p.name != ".dotfiles.update-state"
                    ]
                    self.assertEqual(entries, ["dotfiles"], f"{failure} leaked staging files")
                    if failure == "publication":
                        self.assertIn("changed during bootstrap", res.stderr)
                        retry = run([str(wrapper), "--version"], cwd=fixture)
                        self.assertEqual(retry.returncode, 0, retry.stderr)
                    cached.unlink()

    def test_wrapper_release_pinned_urls(self) -> None:
        content = WRAPPER_SRC.read_text(encoding="utf-8")
        self.assertIn("resolve_release_tag", content)
        self.assertIn("releases/download/$tag", content)
        self.assertNotIn("releases/latest/download", content)

    def test_wrapper_release_repository(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-repo-") as tmp_str:
            fixture = Path(tmp_str)
            repo = fixture / "repo space"
            wrapper = copy_wrapper(repo)
            fake_bin = fixture / "fake-bin"
            write_executable(
                fake_bin / "curl",
                '#!/bin/sh\nprintf \'%s\\n\' "$@" > "$DOTFILES_ROOT/request"\nexit 22\n',
            )
            self.assertEqual(run(["git", "init", "-q", str(repo)], cwd=fixture).returncode, 0)
            self.assertEqual(
                run(
                    [
                        "git",
                        "-C",
                        str(repo),
                        "remote",
                        "add",
                        "origin",
                        "https://github.com/fork-owner/custom-name.git",
                    ],
                    cwd=fixture,
                ).returncode,
                0,
            )

            base_env = dict(os.environ, PATH=f"{fake_bin}:{os.environ.get('PATH', '')}")
            base_env.pop("DOTFILES_RELEASE_REPO", None)

            for remote in (
                "https://github.com/fork-owner/custom-name.git",
                "git@github.com:fork-owner/custom-name.git",
                "ssh://git@github.com/fork-owner/custom-name.git",
            ):
                with self.subTest(remote=remote):
                    run(
                        ["git", "-C", str(repo), "remote", "set-url", "origin", remote],
                        cwd=fixture,
                    )
                    res = run([str(wrapper), "--version"], cwd=fixture, env=base_env)
                    self.assertNotEqual(res.returncode, 0)
                    self.assertIn(
                        "https://api.github.com/repos/fork-owner/custom-name/releases/latest",
                        (repo / "request").read_text(encoding="utf-8").splitlines(),
                    )

            run(
                [
                    "git",
                    "-C",
                    str(repo),
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.com/other/repo",
                ],
                cwd=fixture,
            )
            for override in (
                "publisher/releases",
                "",
                "owner/repo/extra",
                "../repo",
                "owner/repo?query",
            ):
                with self.subTest(override=override):
                    (repo / "request").unlink(missing_ok=True)
                    env = dict(base_env, DOTFILES_RELEASE_REPO=override)
                    res = run([str(wrapper), "--version"], cwd=fixture, env=env)
                    self.assertNotEqual(res.returncode, 0)
                    if override == "publisher/releases":
                        self.assertIn(
                            "https://api.github.com/repos/publisher/releases/releases/latest",
                            (repo / "request").read_text(encoding="utf-8").splitlines(),
                        )
                    else:
                        self.assertFalse((repo / "request").exists())
                        self.assertIn("DOTFILES_RELEASE_REPO", res.stderr)

            (repo / "request").unlink(missing_ok=True)
            res = run([str(wrapper), "--version"], cwd=fixture, env=base_env)
            self.assertNotEqual(res.returncode, 0)
            self.assertFalse((repo / "request").exists())

            run(["git", "-C", str(repo), "remote", "remove", "origin"], cwd=fixture)
            res = run([str(wrapper), "--version"], cwd=fixture, env=base_env)
            self.assertNotEqual(res.returncode, 0)
            self.assertFalse((repo / "request").exists())

    def test_wrapper_api_headers_and_error_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory(prefix="wrapper-api-") as tmp_str:
            fixture = Path(tmp_str)
            wrapper = copy_wrapper(fixture)
            fake_bin = fixture / "fake-bin"
            write_executable(
                fake_bin / "curl",
                "#!/bin/sh\n"
                'printf \'%s\\n\' "$@" > "$DOTFILES_ROOT/argv"\n'
                "if [ ! -t 0 ]; then\n"
                '  cat > "$DOTFILES_ROOT/stdin"\n'
                "fi\n"
                'echo "curl: (22) The requested URL returned error: 403" >&2\n'
                "exit 22\n",
            )
            env = dict(
                os.environ,
                PATH=f"{fake_bin}:{os.environ.get('PATH', '')}",
                DOTFILES_RELEASE_REPO="fixture/releases",
                GH_TOKEN="fixture-tok",
                GITHUB_TOKEN="",
            )
            res = run([str(wrapper), "--version"], cwd=fixture, env=env)
            self.assertNotEqual(res.returncode, 0)
            argv_lines = (fixture / "argv").read_text(encoding="utf-8").splitlines()
            self.assertIn("Accept: application/vnd.github+json", argv_lines)
            self.assertFalse(any("fixture-tok" in line for line in argv_lines))
            stdin_lines = (fixture / "stdin").read_text(encoding="utf-8").splitlines()
            self.assertIn('header = "Authorization: Bearer fixture-tok"', stdin_lines)
            self.assertRegex(res.stderr, re.escape("403"))
