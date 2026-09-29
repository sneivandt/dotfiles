#!/usr/bin/env python3
"""Isolated regressions for managed shell startup and command helpers."""

import http.server
import os
from pathlib import Path
import shutil
import subprocess
import threading
import time
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[4]


class ShellConfigTests(unittest.TestCase):
    def setUp(self):
        self.fixture = ROOT / (".shell-config-test-" + uuid.uuid4().hex)
        self.fixture.mkdir()
        self.addCleanup(shutil.rmtree, self.fixture)
        self.home = self.fixture / "home with spaces"
        self.home.mkdir()
        self.env = {
            "HOME": str(self.home),
            "PATH": "/usr/bin:/bin",
            "TERM": "xterm-256color",
            "LC_ALL": "C",
            "SHELL": "/usr/bin/zsh",
            "ROOT": str(ROOT),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_COUNT": "0",
        }

    def write(self, path, contents, executable=False):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
        if executable:
            path.chmod(0o755)
        return path

    def run_shell(self, script, shell="zsh", interactive=False, expected=0):
        args = (["bash", "--noprofile", "--norc", "-ic" if interactive else "-c"]
                if shell == "bash" else ["zsh", "-dfi" if interactive else "-df", "-c"])
        result = subprocess.run(
            args + [script], cwd=self.fixture, env=self.env,
            text=True, capture_output=True, timeout=20,
        )
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def install_profile(self):
        directory = self.home / ".config/shell"
        directory.mkdir(parents=True)
        for name in ("profile.sh", "path.sh"):
            (directory / name).symlink_to(ROOT / "symlinks/config/shell" / name)
        self.write(self.home / ".local/bin/nvim", "#!/bin/sh\nexit 0\n", executable=True)

    def prepare_completion(self):
        cache = self.home / ".cache/zsh"
        self.write(cache / "hosts.cache", "typeset -ga hosts=(localhost)\n")
        self.env["ZSH_COMPDUMP"] = str(cache / "zcompdump-fixture")
        self.write(
            self.home / ".config/zsh/completions/_dotfiles",
            "source <(DOTFILES_COMPLETE=zsh dotfiles)\n",
        )
        binary = self.write(
            self.fixture / "bin/dotfiles",
            "#!/bin/sh\n"
            '[ "$DOTFILES_COMPLETE" = zsh ] || exit 1\n'
            "printf '%s\\n' '_clap_dynamic_completer_dotfiles() { return 0; }' "
            "'compdef _clap_dynamic_completer_dotfiles dotfiles'\n",
            executable=True,
        )
        self.env["PATH"] = str(binary.parent) + ":/usr/bin:/bin"
        return Path(self.env["ZSH_COMPDUMP"])

    def test_login_path_precedes_capability_checks_and_is_idempotent(self):
        self.install_profile()
        for shell, profile in (("bash", "bash_profile"), ("zsh", "zprofile")):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    f'. "$ROOT/symlinks/{profile}"\n'
                    'initial_path=$PATH\n'
                    '. "$HOME/.config/shell/path.sh"\n'
                    '[ "$PATH" = "$initial_path" ] || exit 1\n'
                    '[ "$EDITOR" = nvim ] || exit 1\n'
                    '[ "$(command -v nvim)" = "$HOME/.local/bin/nvim" ] || exit 1\n'
                    'printf "%s\\n" "$PATH"\n',
                    shell=shell,
                )
                paths = result.stdout.strip().split(":")
                for suffix in (".local/bin", ".bin", "src/go/bin", ".cargo/bin"):
                    self.assertEqual(paths.count(str(self.home / suffix)), 1)

    def test_uwsm_inherits_login_path(self):
        self.install_profile()
        self.write(
            self.home / ".local/bin/uwsm",
            "#!/bin/sh\n"
            'case "$1" in check) exit 0;; start) printf "%s\\n" "$PATH";; esac\n',
            executable=True,
        )
        result = self.run_shell(
            'tty() { [ "$1" = -s ] || printf "/dev/tty1\\n"; }\n'
            'source "$ROOT/symlinks/zprofile"\n'
            'exit 99\n'
        )
        self.assertIn(str(self.home / ".local/bin"), result.stdout.strip().split(":"))

    def test_interactive_shell_refreshes_gpg_tty(self):
        self.env["GPG_TTY"] = "/dev/pts/inherited"
        for shell, rc in (("bash", "bashrc"), ("zsh", "zshrc")):
            for has_tty in (True, False):
                with self.subTest(shell=shell, has_tty=has_tty):
                    tty = ('tty() { [ "$1" = -s ] || printf "/dev/pts/current\\n"; }\n'
                           if has_tty else "tty() { return 1; }\n")
                    expected = "/dev/pts/current" if has_tty else "/dev/pts/inherited"
                    self.run_shell(
                        tty + f'. "$ROOT/symlinks/{rc}"\n'
                        f'[ "$GPG_TTY" = "{expected}" ]\n',
                        shell=shell, interactive=True,
                    )

    def test_completion_registers_on_cold_and_warm_start(self):
        self.prepare_completion()
        for _ in range(2):
            self.run_shell(
                'setopt extendedglob\n'
                'source "$ROOT/symlinks/config/zsh/completion.zsh"\n'
                '[[ ${_comps[dotfiles]} == _clap_dynamic_completer_dotfiles ]]\n'
            )

    def test_completion_without_generated_file(self):
        self.prepare_completion()
        (self.home / ".config/zsh/completions/_dotfiles").unlink()
        self.run_shell(
            'setopt extendedglob\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh"\n'
            '(( ! ${+_comps[dotfiles]} ))\n'
        )

    def test_completion_scan_age_does_not_depend_on_dump_changes(self):
        dump = self.prepare_completion()
        script = ('setopt extendedglob\n'
                  'source "$ROOT/symlinks/config/zsh/completion.zsh"\n')
        self.run_shell(script)
        checked = Path(str(dump) + ".checked")
        old = int(time.time() - 172800)
        os.utime(dump, (old, old))
        os.utime(checked, (old, old))
        self.run_shell(script)
        self.assertEqual(int(dump.stat().st_mtime), old)
        self.assertGreater(checked.stat().st_mtime, old)
        scan_time = checked.stat().st_mtime_ns
        self.run_shell(script)
        self.assertEqual(checked.stat().st_mtime_ns, scan_time)
        dump.unlink()
        self.run_shell(script)
        self.assertTrue(dump.is_file(), "a recent scan marker must not hide a missing dump")

    def test_failed_completion_scan_is_not_marked_fresh(self):
        dump = self.prepare_completion()
        self.run_shell(
            'setopt extendedglob\n'
            'autoload() { :; }\n'
            'compinit() { return 17; }\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh"\n',
            expected=17,
        )
        self.assertFalse(Path(str(dump) + ".checked").exists())

    def test_docker_clean_preserves_each_failure(self):
        for failed in ("container", "network", "image", "none"):
            with self.subTest(failed=failed):
                result = self.run_shell(
                    'docker() { print -r -- "$1"; [[ $1 != "$FAILED" ]] || return 23; }\n'
                    f'FAILED={failed}\n'
                    'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
                    'autoload -Uz docker-clean\n'
                    'docker-clean\n',
                    expected=0 if failed == "none" else 23,
                )
                stages = ["container", "network", "image"]
                expected = stages if failed == "none" else stages[:stages.index(failed) + 1]
                self.assertEqual(result.stdout.splitlines(), expected)

    def test_ff_requires_editor_only_when_editing(self):
        file = self.write(self.fixture / "selected file", "fixture\n")
        self.env["SELECTED"] = str(file)
        self.env["EDITOR"] = "missing-editor-for-fixture"
        helpers = (
            'fd() { print -r -- "$SELECTED"; }\n'
            'fzf() { cat; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\n'
        )
        result = self.run_shell(helpers + "ff selected\n")
        self.assertEqual(result.stdout.strip(), str(file))
        result = self.run_shell(helpers + "ff -e selected\n", expected=1)
        self.assertIn("missing-editor-for-fixture not installed", result.stderr)
        self.env["EDITOR"] = "fixture-editor"
        result = self.run_shell(
            helpers + 'fixture-editor() { print -r -- "edited: $1"; }\nff -e selected\n'
        )
        self.assertEqual(result.stdout.strip(), "edited: " + str(file))

    def test_windows_git_editor_uses_installed_insiders_command(self):
        result = subprocess.run(
            ["git", "config", "--file", str(ROOT / "symlinks/config/git/windows"),
             "--get", "core.editor"],
            cwd=self.fixture, env=self.env, text=True, capture_output=True, check=True,
        )
        self.assertEqual(result.stdout.strip(), "code-insiders --wait")

    def test_wget_does_not_append_changed_content_without_modification_date(self):
        payload = b"NEW-CONTENT-LONGER\n"
        ranges = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_HEAD(self):
                self.respond(False)

            def do_GET(self):
                self.respond(True)

            def respond(self, body):
                requested = self.headers.get("Range")
                ranges.append(requested)
                start = int(requested.split("=")[1].split("-")[0]) if requested else 0
                content = payload[start:]
                self.send_response(206 if requested else 200)
                self.send_header("Content-Length", str(len(content)))
                if requested:
                    self.send_header("Content-Range", f"bytes {start}-{len(payload)-1}/{len(payload)}")
                self.end_headers()
                if body:
                    self.wfile.write(content)

            def log_message(self, *args):
                pass

        server = http.server.HTTPServer(("localhost", 0), Handler)
        thread = threading.Thread(target=server.serve_forever)
        thread.start()
        try:
            self.write(self.fixture / "release.bin", "OLD\n")
            self.env["WGETRC"] = str(ROOT / "symlinks/config/wgetrc")
            subprocess.run(
                ["wget", "-q", f"http://localhost:{server.server_port}/release.bin"],
                cwd=self.fixture, env=self.env, check=True, timeout=10,
            )
            downloads = [path.read_bytes() for path in self.fixture.glob("release.bin*")]
            self.assertIn(payload, downloads)
            self.assertTrue(all(content in (b"OLD\n", payload) for content in downloads))
            self.assertTrue(ranges)
            self.assertTrue(all(value is None for value in ranges))
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == "__main__":
    unittest.main()
