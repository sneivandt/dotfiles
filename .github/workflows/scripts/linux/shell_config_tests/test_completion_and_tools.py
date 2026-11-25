"""Completion cache, fzf/ff, clipboard, git ignore, and tmux tests."""

import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import time

from shell_config_tests.base import ROOT, ShellConfigTestCase


class CompletionAndToolsTests(ShellConfigTestCase):
    def test_completion_registers_on_cold_and_warm_start(self):
        self.prepare_completion()
        for _ in range(2):
            self.run_shell(
                'setopt extendedglob\n'
                'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
                '[[ ${_comps[dotfiles]} == _dotfiles_complete ]]\n'
            )

    def test_dot_explain_completes_home_path_without_escaping_tilde(self):
        self.prepare_completion()
        self.write(self.home / ".bashrc", "")
        self.env["ZDOTDIR"] = str(self.home)
        self.write(
            self.home / ".zshrc",
            'setopt extendedglob\n'
            'completion_dirs=(${^fpath}/compinit(N:h))\n'
            'fpath=("$completion_dirs[1]" "$completion_dirs[1]"/*(/N))\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
            'alias dot=dotfiles\n'
            'PS1="READY>"\n'
            'capture-buffer() { print -r -- "$BUFFER" > "$HOME/completed"; exit; }\n'
            'zle -N capture-buffer\n'
            'bindkey "^G" capture-buffer\n',
        )
        self.run_shell(
            'zmodload zsh/zpty || exit $?\n'
            'zpty fixture zsh -di || exit $?\n'
            'zpty -r fixture output "*READY>*" || exit $?\n'
            "zpty -w -n fixture $'dot explain ~/.bash\\t\\x07' || exit $?\n"
            'while zpty -r fixture output; do :; done\n'
        )
        self.assertEqual((self.home / "completed").read_text(), "dot explain ~/.bashrc \n")

    def test_completion_without_generated_file(self):
        self.prepare_completion()
        (self.home / ".config/zsh/completions/_dotfiles").unlink()
        self.run_shell(
            'setopt extendedglob\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
            '(( ! ${+_comps[dotfiles]} ))\n'
        )

    def test_completion_fixture_ignores_unrelated_insecure_vendor_tree(self):
        self.prepare_completion()
        vendor = self.fixture / "vendor-completions"
        vendor.mkdir()
        vendor.chmod(0o777)
        self.write(vendor / "_fixture", "#compdef fixture\n")
        native = subprocess.check_output(
            ["zsh", "-df", "-c", "print -r -- ${(j.:.)fpath}"],
            env=self.env, text=True, timeout=10,
        ).strip()
        self.env["FPATH"] = str(vendor) + ":" + native
        self.run_shell(
            'setopt extendedglob\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
            '[[ ${_comps[dotfiles]} == _dotfiles_complete ]]\n'
        )

    def test_completion_scan_age_does_not_depend_on_dump_changes(self):
        dump = self.prepare_completion()
        script = ('setopt extendedglob\n'
                  'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n')
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

    def test_hostname_cache_refreshes_after_ssh_source_changes(self):
        for source, contents, hostnames in (
            ("config", "Host new-config-alias\n  HostName example.test\n", ["new-config-alias"]),
            (
                "known_hosts",
                "new-known-host,another-known-host\tssh-ed25519 fixture\n"
                "|1|salt|hash ssh-ed25519 fixture\n",
                ["new-known-host", "another-known-host"],
            ),
        ):
            with self.subTest(source=source):
                if "ZSH_COMPDUMP" not in self.env:
                    self.prepare_completion()
                cache = self.home / ".cache/zsh/hosts.cache"
                changed = self.write(self.home / ".ssh" / source, "")
                script = (
                    'setopt extendedglob\n'
                    'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
                    'print -rl -- "${hosts[@]}"\n'
                )
                self.run_shell(script)
                self.write(changed, contents)
                stamp = int(time.time()) - 20
                os.utime(cache, (stamp, stamp))
                os.utime(changed, (stamp + 2, stamp + 2))
                result = self.run_shell(script)
                candidates = result.stdout.splitlines()
                for hostname in hostnames:
                    self.assertIn(hostname, candidates)
                self.assertNotIn("|1|salt|hash", candidates)
                updated = cache.stat().st_mtime_ns
                self.run_shell(script)
                self.assertEqual(cache.stat().st_mtime_ns, updated,
                                 "unchanged sources should reuse the refreshed cache")
                changed.unlink()
                candidates = self.run_shell(script).stdout.splitlines()
                for hostname in hostnames:
                    self.assertNotIn(hostname, candidates)

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
            'fd() { printf "%s\\0" "$SELECTED"; }\n'
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

    def test_ff_supports_editor_arguments_without_evaluating_shell_code(self):
        selected = self.write(self.fixture / "selected file", "fixture\n")
        output = self.fixture / "editor-arguments"
        editor = self.write(
            self.fixture / "bin with spaces/editor",
            '#!/usr/bin/python3\nimport json, os, sys\n'
            'with open(os.environ["EDITED_ARGS"], "w") as stream:\n'
            '    json.dump(sys.argv[1:], stream)\n', executable=True,
        )
        self.env.update(
            SELECTED=str(selected), EDITED_ARGS=str(output),
            EDITOR=shlex.quote(str(editor)) + ' --wait "two words" \'$(touch injected)\'',
        )
        self.run_shell(
            'fd() { printf "%s\\0" "$SELECTED"; }\n'
            'fzf() { cat; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\nff -e selected\n'
        )
        self.assertEqual(json.loads(output.read_text()),
                         ["--wait", "two words", "$(touch injected)", str(selected)])
        self.assertFalse((self.fixture / "injected").exists())

    def test_ff_prefixes_option_like_filenames_before_editing(self):
        self.write(self.fixture / "-selected", "fixture\n")
        self.env.update(SELECTED="-selected", EDITOR="fixture-editor")
        helpers = (
            'fd() { printf "%s\\0" "$SELECTED"; }\n'
            'fzf() { cat; }\n'
            'fixture-editor() { print -r -- "$1"; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\n'
        )
        result = self.run_shell(helpers + 'ff -e selected\n')
        self.assertEqual(result.stdout, "./-selected\n")
        result = self.run_shell(helpers + 'ff selected\n')
        self.assertEqual(result.stdout, "-selected\n")

    def test_fzf_file_preview_delimits_option_like_filenames(self):
        bat = shutil.which("bat") or shutil.which("batcat")
        self.assertIsNotNone(bat, "bat or batcat is required for preview coverage")
        # Debian/Ubuntu package the executable as batcat; exercise the configured
        # bat command through a fixture wrapper on both distributions.
        wrapper = self.write(
            self.fixture / "bin/bat", '#!/bin/sh\nexec "$BAT_BINARY" "$@"\n', executable=True,
        )
        self.env.update(BAT_BINARY=bat, PATH=str(wrapper.parent) + ":" + self.env["PATH"])
        self.write(self.fixture / "-preview", "preview fixture content\n")
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    'fd() { :; }\n'
                    '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                    "python3 - <<'PY'\n"
                    'import os, shlex, subprocess\n'
                    'preview = shlex.split(os.environ["FZF_CTRL_T_OPTS"])[1]\n'
                    'subprocess.run(preview.replace("{}", shlex.quote("-preview")),\n'
                    '               shell=True, check=True)\n'
                    'PY\n', shell=shell,
                )
                self.assertIn("preview fixture content", result.stdout)

    def test_clipboard_aliases_choose_available_session_backend(self):
        binary_dir = self.fixture / "bin"
        clipboard = self.fixture / "clipboard"
        calls = self.fixture / "clipboard-calls"
        backend = (
            "#!/usr/bin/python3\n"
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "name = Path(sys.argv[0]).name\n"
            "with open(os.environ['CLIPBOARD_CALLS'], 'a') as log:\n"
            "    log.write(json.dumps([name, *sys.argv[1:]]) + '\\n')\n"
            "clipboard = Path(os.environ['CLIPBOARD'])\n"
            "if name == 'wl-paste' or '-out' in sys.argv:\n"
            "    sys.stdout.buffer.write(clipboard.read_bytes())\n"
            "else:\n"
            "    clipboard.write_bytes(sys.stdin.buffer.read())\n"
        )
        self.env.update(PATH=str(binary_dir) + ":/usr/bin:/bin",
                        CLIPBOARD=str(clipboard), CLIPBOARD_CALLS=str(calls),
                        CLIPBOARD_BIN=str(binary_dir))
        for shell in ("bash", "zsh"):
            for wayland, tools, expected in (
                (True, ("wl-copy", "wl-paste", "xclip"), "wl-copy"),
                (True, ("wl-copy", "wl-paste"), "wl-copy"),
                (False, ("wl-copy", "wl-paste", "xclip"), "xclip"),
                (True, ("wl-copy", "xclip"), "xclip"),
            ):
                with self.subTest(shell=shell, wayland=wayland, tools=tools):
                    for name in ("wl-copy", "wl-paste", "xclip"):
                        binary = binary_dir / name
                        if name in tools:
                            self.write(binary, backend, executable=True)
                        else:
                            binary.unlink(missing_ok=True)
                    calls.unlink(missing_ok=True)
                    self.env["WAYLAND_DISPLAY"] = "wayland-fixture" if wayland else ""
                    # Hide host tools while keeping the fixture capabilities real.
                    result = self.run_shell(
                        ('shopt -s expand_aliases\n' if shell == "bash" else "") +
                        'command() {\n'
                        '  if [ "$1" = -v ]; then\n'
                        '    case "$2" in wl-copy|wl-paste|xclip) [ -x "$CLIPBOARD_BIN/$2" ]; return $?;; esac\n'
                        '  fi\n'
                        '  builtin command "$@"\n'
                        '}\n'
                        '. "$ROOT/symlinks/config/shell/aliases.sh"\n'
                        'eval \'printf %s "clipboard without trailing newline" | pbcopy\'\n'
                        'eval pbpaste\n', shell=shell,
                    )
                    self.assertEqual(result.stdout, "clipboard without trailing newline")
                    invocations = [json.loads(line) for line in calls.read_text().splitlines()]
                    self.assertEqual(invocations[0][0], expected)
                    if expected == "wl-copy":
                        self.assertEqual(invocations[1], ["wl-paste", "--no-newline"])

    def test_global_git_ignore_keeps_log_fixtures_visible(self):
        subprocess.run(["git", "init", "-q", str(self.fixture)],
                       env=self.env, check=True)
        result = subprocess.run(
            ["git", "-c", "core.excludesfile=" + str(ROOT / "symlinks/config/git/ignore"),
             "check-ignore", "--no-index", "tests/fixtures/parser.log"],
            cwd=self.fixture, env=self.env, text=True, capture_output=True,
        )
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)

    def test_ff_edits_exact_filename_with_embedded_and_trailing_newlines(self):
        self.assertIsNotNone(shutil.which("fzf"), "fzf is required for selection coverage")
        selected = self.write(self.fixture / "needle\nreport\\literal.txt\n", "fixture\n")
        output = self.fixture / "edited-path"
        self.env.update(SELECTED=str(selected), EDITED_PATH=str(output),
                        EDITOR="fixture-editor", FZF_DEFAULT_OPTS="--filter=needle")
        helpers = (
            'fd() { printf "%s\\0" "$SELECTED"; }\n'
            'locate() { printf "%s\\0" "$SELECTED"; }\n'
            'fixture-editor() { printf %s "$1" > "$EDITED_PATH"; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\n'
        )
        for backend_name in ("fd", "locate"):
            with self.subTest(backend=backend_name):
                select_backend = (
                    'command() {\n'
                    '  [[ "$1" == -v && "$2" == fd ]] && return 1\n'
                    '  builtin command "$@"\n'
                    '}\n'
                ) if backend_name == "locate" else ""
                self.run_shell(helpers + select_backend + 'ff -e needle\n')
                self.assertEqual(output.read_text(), str(selected))
                result = self.run_shell(helpers + select_backend + 'ff needle\n')
                self.assertEqual(result.stdout, str(selected) + "\n")

    def test_ff_requires_exactly_one_nonempty_query(self):
        helpers = (
            'fd() { print -r -- "search must not run"; }\n'
            'fzf() { cat; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\n'
        )
        for args in ("", "''", "needle extra", "-e needle extra"):
            with self.subTest(args=args):
                result = self.run_shell(helpers + f"ff {args}\n", expected=1)
                self.assertIn("Usage: ff", result.stdout)
                self.assertNotIn("search must not run", result.stdout)

    def test_ff_propagates_pipeline_failures_without_changing_caller_options(self):
        for backend in ("fd", "locate"):
            for failed in ("search", "selection"):
                with self.subTest(backend=backend, failed=failed):
                    result = self.run_shell(
                        'unsetopt pipefail\n'
                        'fd() { printf "partial-match\\0"; return ' +
                        ('23' if failed == "search" else '0') + '; }\n'
                        'locate() { fd "$@"; }\n'
                        'fzf() { cat; return ' + ('29' if failed == "selection" else '0') + '; }\n'
                        'command() {\n'
                        '  [[ "$1" == -v && "$2" == fd && "$BACKEND" == locate ]] && return 1\n'
                        '  builtin command "$@"\n'
                        '}\n'
                        f'BACKEND={backend}\n'
                        'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
                        'autoload -Uz ff\n'
                        'ff needle\n'
                        'result=$?\n'
                        '[[ -o pipefail ]] && exit 99\n'
                        'exit "$result"\n', expected=23 if failed == "search" else 29,
                    )
                    self.assertEqual(result.stdout, "")

    def test_ff_cancel_prioritizes_fzf_130_over_fd_broken_pipe(self):
        result = self.run_shell(
            'fd() { return 1; }\n'
            'fzf() { return 130; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\n'
            'ff needle\n',
            expected=130,
        )
        self.assertEqual(result.stdout, "")

    def test_ff_keeps_option_like_queries_as_search_patterns(self):
        backend = self.write(
            self.fixture / "search.py",
            "import argparse\n"
            "parser = argparse.ArgumentParser()\n"
            "parser.add_argument('--version', action='version', version='backend version')\n"
            "parser.add_argument('--type')\n"
            "parser.add_argument('-0', action='store_true')\n"
            "parser.add_argument('query')\n"
            "args = parser.parse_args()\n"
            "print(args.query, end='\\0' if getattr(args, '0') else '\\n')\n",
        )
        self.env["SEARCH_BACKEND"] = str(backend)
        helpers = (
            'fd() { python3 "$SEARCH_BACKEND" "$@"; }\n'
            'locate() { python3 "$SEARCH_BACKEND" "$@"; }\n'
            'fzf() { cat; }\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ff\n'
        )
        for backend_name in ("fd", "locate"):
            for query in ("--version", "-needle", "two words"):
                with self.subTest(backend=backend_name, query=query):
                    self.env["QUERY"] = query
                    select_backend = (
                        'command() {\n'
                        '  [[ "$1" == -v && "$2" == fd ]] && return 1\n'
                        '  builtin command "$@"\n'
                        '}\n'
                    ) if backend_name == "locate" else ""
                    result = self.run_shell(helpers + select_backend + 'ff -- "$QUERY"\n')
                    self.assertEqual(result.stdout.strip(), query)

    def test_tmux_copy_bindings_choose_the_available_session_backend(self):
        if shutil.which("tmux") is None:
            self.skipTest("tmux is not available")
        command = ["tmux", "-S", str(self.fixture / "tmux.sock"),
                   "-f", str(ROOT / "symlinks/config/tmux/tmux.conf")]
        try:
            subprocess.run(command + ["new-session", "-d", "-s", "fixture", "sleep 60"],
                           env=self.env, check=True, capture_output=True, timeout=10)
            result = subprocess.run(command + ["list-keys", "-T", "copy-mode-vi"],
                                    env=self.env, check=True, text=True,
                                    capture_output=True, timeout=10)
            keys = ("y", "C-c", "MouseDragEnd1Pane")
            bindings = {}
            for line in result.stdout.splitlines():
                parts = shlex.split(line)
                if parts[3] in keys and "copy-pipe-and-cancel" in parts:
                    bindings[parts[3]] = line
            self.assertCountEqual(bindings, keys)

            binary_dir = self.fixture / "bin"
            binary_dir.mkdir()
            (binary_dir / "cat").symlink_to(shutil.which("cat"))
            clipboard = self.fixture / "clipboard"
            calls = self.fixture / "clipboard-calls"
            self.env.update(PATH=str(binary_dir), CLIPBOARD=str(clipboard),
                            CLIPBOARD_CALLS=str(calls))
            for wayland, tools, expected in (
                (True, ("wl-copy", "xclip"), "wl-copy"),
                (False, ("wl-copy", "xclip"), "xclip"),
                (True, ("xclip",), "xclip"),
                (False, ("wl-copy",), None),
            ):
                self.env["WAYLAND_DISPLAY"] = "wayland-fixture" if wayland else ""
                for name in ("wl-copy", "xclip"):
                    binary = binary_dir / name
                    if name in tools:
                        self.write(
                            binary,
                            f'#!/bin/sh\nprintf "%s\\n" "{name}" "$@" > "$CLIPBOARD_CALLS"\n'
                            'cat > "$CLIPBOARD"\n',
                            executable=True,
                        )
                    else:
                        binary.unlink(missing_ok=True)
                for key, invocation in bindings.items():
                    with self.subTest(key=key, wayland=wayland, tools=tools):
                        clipboard.unlink(missing_ok=True)
                        calls.unlink(missing_ok=True)
                        # Decode tmux's quoting with Bash, including escaped dollar signs.
                        script = 'bind-key() { shift 6; /bin/sh -c "$1"; }\n' + invocation
                        subprocess.run(["/bin/bash", "--noprofile", "--norc", "-c", script],
                                       input=b"selection without trailing newline",
                                       env=self.env, check=True, capture_output=True, timeout=10)
                        if expected is None:
                            self.assertFalse(calls.exists())
                            self.assertFalse(clipboard.exists())
                        else:
                            arguments = ["-selection", "clipboard", "-in"] if expected == "xclip" else []
                            self.assertEqual(calls.read_text().splitlines(), [expected, *arguments])
                            self.assertEqual(clipboard.read_bytes(), b"selection without trailing newline")
        finally:
            subprocess.run([shutil.which("tmux"), "-S", str(self.fixture / "tmux.sock"),
                            "kill-server"], env=self.env, capture_output=True, timeout=10)

    def test_tmux_panes_load_login_profile_without_starting_the_desktop(self):
        if shutil.which("tmux") is None:
            self.skipTest("tmux is not available")
        self.install_profile()
        self.env["LOGIN_MARKER"] = str(self.fixture / "login-profile-loaded")
        self.env["DESKTOP_MARKER"] = str(self.fixture / "desktop-started")
        self.env["SHELL"] = "/usr/bin/zsh"
        self.write(
            self.home / ".zprofile",
            'source "$ROOT/symlinks/zprofile"\n'
            'print -r -- "$EDITOR" > "$LOGIN_MARKER"\n'
            'exec /usr/bin/sleep 60\n',
        )
        self.write(self.home / ".local/bin/uwsm",
                   '#!/bin/sh\nprintf invoked > "$DESKTOP_MARKER"\nexit 99\n',
                   executable=True)
        # A private socket and fixture HOME keep existing tmux sessions untouched.
        command = ["tmux", "-S", str(self.fixture / "tmux.sock"),
                   "-f", str(ROOT / "symlinks/config/tmux/tmux.conf")]
        try:
            result = subprocess.run(command + ["new-session", "-d", "-s", "fixture"],
                                    env=self.env, text=True, capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            marker = Path(self.env["LOGIN_MARKER"])
            deadline = time.monotonic() + 5
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertTrue(marker.exists(), "new panes must load the login profile")
            self.assertEqual(marker.read_text().strip(), "nvim")
            self.assertFalse(Path(self.env["DESKTOP_MARKER"]).exists())
        finally:
            subprocess.run(command + ["kill-server"], env=self.env,
                           capture_output=True, timeout=10)

    def test_tmux_terminal_has_word_navigation_bindings(self):
        for term in ("screen-256color", "tmux-256color", "xterm-256color"):
            with self.subTest(term=term):
                self.env["TERM"] = term
                result = self.run_shell(
                    'source "$ROOT/symlinks/config/zsh/key-bindings.zsh"\n'
                    'bindkey "\\e[1;5D"\n'
                    'bindkey "\\e[1;5C"\n'
                    'bindkey "\\e[3;5~"\n'
                )
                self.assertEqual([line.split()[-1] for line in result.stdout.splitlines()],
                                 ["backward-word", "forward-word", "kill-word"])
