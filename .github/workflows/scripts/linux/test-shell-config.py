#!/usr/bin/env python3
"""Isolated regressions for managed shell startup and command helpers."""

import gzip
import json
import http.server
import os
import pty
from pathlib import Path
import shutil
import select
import shlex
import configparser
import struct
import subprocess
import threading
import time
import unittest
import uuid


ROOT = Path(__file__).resolve().parents[4]


class ShellConfigTests(unittest.TestCase):
    def setUp(self):
        self.fixture = ROOT / (".shell-config-test-" + uuid.uuid4().hex)
        self.fixture.mkdir(mode=0o700)
        self.addCleanup(shutil.rmtree, self.fixture)
        self.home = self.fixture / "home with spaces"
        self.home.mkdir(mode=0o700)
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
        path.chmod(0o755 if executable else 0o644)
        return path

    def run_shell(self, script, shell="zsh", interactive=False, expected=0):
        args = (["bash", "--noprofile", "--norc", "-ic" if interactive else "-c"]
                if shell == "bash" else ["zsh", "-dfi" if interactive else "-df", "-c"])
        if shell == "zsh" and "ZSH_COMPDUMP" in self.env:
            # Test the real completion initializer, not optional host/vendor
            # completion trees with runner-specific ownership.
            script = 'fpath=(${^fpath}/compinit(N:h))\n' + script
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
        # Keep compaudit's trust checks enabled even with a permissive CI umask.
        completion_dir = self.home / ".config/zsh/completions"
        for directory in (completion_dir.parent.parent, completion_dir.parent, completion_dir):
            directory.mkdir(mode=0o700, exist_ok=True)
        self.write(
            completion_dir / "_dotfiles",
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

    def test_path_moves_existing_user_directories_ahead_of_system_commands(self):
        user_paths = [str(self.home / suffix) for suffix in
                      (".local/bin", ".bin", "src/go/bin", ".cargo/bin")]
        inherited = ["/usr/bin", user_paths[3], "", "/literal [*] path", "/bin",
                     user_paths[0], user_paths[3], ""]
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                self.env["PATH"] = ":".join(inherited)
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/path.sh"\n'
                    'initial_path=$PATH\n'
                    '. "$ROOT/symlinks/config/shell/path.sh"\n'
                    '[ "$PATH" = "$initial_path" ] || exit 1\n'
                    'printf "%s\\n" "$PATH"\n', shell=shell,
                )
                self.assertEqual(result.stdout.rstrip("\n").split(":"),
                                 user_paths + [part for part in inherited if part not in user_paths])

    def test_qt_backend_respects_overrides_and_session(self):
        for shell in ("bash", "zsh"):
            for display, backend, expected in (
                ("wayland-1", "offscreen", "offscreen"),
                ("wayland-1", "xcb", "xcb"),
                ("wayland-1", None, "wayland;xcb"),
                (None, None, "unset"),
            ):
                with self.subTest(shell=shell, display=display, backend=backend):
                    self.env.pop("WAYLAND_DISPLAY", None)
                    self.env.pop("QT_QPA_PLATFORM", None)
                    if display is not None:
                        self.env["WAYLAND_DISPLAY"] = display
                    if backend is not None:
                        self.env["QT_QPA_PLATFORM"] = backend
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile-arch-desktop.sh"\n'
                        'printf "%s\\n" "${QT_QPA_PLATFORM-unset}"\n', shell=shell,
                    )
                    self.assertEqual(result.stdout.strip(), expected)

    def test_profile_preserves_locale_and_scoped_linter_preferences(self):
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                self.env.update(LANG="C.UTF-8", LANGUAGE="fr:en", SHELLCHECK_OPTS="-e SC2034")
                self.run_shell(
                    '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                    '[ "$LANG" = C.UTF-8 ] && [ "$LANGUAGE" = fr:en ]\n'
                    '[ "$SHELLCHECK_OPTS" = "-e SC2034" ]\n', shell=shell,
                )
                for variable in ("LANG", "LANGUAGE", "SHELLCHECK_OPTS"):
                    self.env.pop(variable)
                self.run_shell(
                    '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                    '[ "$LANG" = en_US.UTF-8 ]\n'
                    '[ "${LANGUAGE+x}" != x ] && [ "${SHELLCHECK_OPTS+x}" != x ]\n',
                    shell=shell,
                )

    def test_history_retains_duplicate_expiration_headroom(self):
        result = self.run_shell(
            'source "$ROOT/symlinks/zshrc"\n'
            '[[ -o hist_expire_dups_first ]] && (( HISTSIZE > SAVEHIST ))\n'
            'print -r -- "$SAVEHIST"\n', interactive=True,
        )
        self.assertEqual(result.stdout.strip(), "100000")

    def test_process_completion_selects_only_the_current_user(self):
        self.prepare_completion()
        self.env["USER"] = "fixture user"
        result = self.run_shell(
            'setopt extendedglob\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
            'zstyle -s ":completion:fixture:processes" command invocation || exit 1\n'
            'ps() { print -rl -- "$@"; }\n'
            'eval "$invocation"\n',
        )
        self.assertEqual(result.stdout.splitlines(), ["-u", "fixture user", "-o", "pid,tty,time,args"])

    def test_git_alias_listing_excludes_similarly_named_sections(self):
        subprocess.run(["git", "init", "-q", str(self.fixture)], env=self.env, check=True)
        command = ["git", "-c", "include.path=" + str(ROOT / "symlinks/config/git/aliases"),
                   "-c", "aliasx.fixture=unexpected", "-c", "alias.fixture=log --oneline"]
        result = subprocess.run(command + ["alias"], cwd=self.fixture, env=self.env,
                                text=True, capture_output=True, check=True)
        self.assertIn("fixture=log --oneline", result.stdout.splitlines())
        self.assertNotIn("unexpected", result.stdout)
        self.assertEqual(result.stdout.splitlines(), sorted(result.stdout.splitlines()))

    def test_shared_vscode_snippets_are_visible_but_local_state_is_ignored(self):
        subprocess.run(["git", "init", "-q", str(self.fixture)], env=self.env, check=True)
        command = ["git", "-c", "core.excludesfile=" + str(ROOT / "symlinks/config/git/ignore"),
                   "check-ignore", "--no-index"]
        for name, ignored in ((".vscode/project.code-snippets", False),
                              (".vscode/local-state.json", True)):
            with self.subTest(name=name):
                result = subprocess.run(command + [name], cwd=self.fixture, env=self.env,
                                        text=True, capture_output=True)
                self.assertEqual(result.returncode, 0 if ignored else 1, result.stderr)

    def test_keymaps_are_stable_across_editors_and_terminal_capabilities(self):
        for editor in ("nvim", "nano"):
            for term in ("xterm-256color", "screen-256color", "tmux-256color", "linux"):
                with self.subTest(editor=editor, term=term):
                    self.env.update(EDITOR=editor, VISUAL=editor, TERM=term)
                    result = self.run_shell(
                        'source "$ROOT/symlinks/config/zsh/key-bindings.zsh"\n'
                        'bindkey -lL main\n'
                        'bindkey "^A"; bindkey "^E"\n'
                        'for capability in khome kend kdch1; do\n'
                        '  [[ -z ${terminfo[$capability]} ]] || bindkey "${terminfo[$capability]}"\n'
                        'done\n', interactive=True,
                    )
                    lines = result.stdout.splitlines()
                    self.assertEqual(lines[0], "bindkey -A emacs main")
                    self.assertEqual([line.split()[-1] for line in lines[1:3]],
                                     ["beginning-of-line", "end-of-line"])
                    self.assertTrue(all(line.split()[-1] in
                                        ("beginning-of-line", "end-of-line", "delete-char")
                                        for line in lines[3:]))
        result = self.run_shell(
            'zmodload() { return 1; }\n'
            'source "$ROOT/symlinks/config/zsh/key-bindings.zsh"\n'
            'bindkey "\\e[H"; bindkey "\\e[F"; bindkey "\\e[3~"\n',
        )
        self.assertEqual([line.split()[-1] for line in result.stdout.splitlines()],
                         ["beginning-of-line", "end-of-line", "delete-char"])

    def test_desktop_profile_paths_and_overrides(self):
        config = self.home / "custom config"
        cache = self.home / "custom cache"
        self.env.update(XDG_CONFIG_HOME=str(config), XDG_CACHE_HOME=str(cache))
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/profile-desktop.sh"\n'
                    'printf "%s\\n" "${GTK2_RC_FILES-unset}" "$__GL_SHADER_DISK_CACHE_PATH"\n',
                    shell=shell,
                )
                self.assertEqual(result.stdout.splitlines(), ["unset", str(cache / "GLCache")])
                gtk = self.write(config / "gtk-2.0/gtkrc-2.0", "fixture\n")
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/profile-desktop.sh"\n'
                    'printf "%s\\n" "$GTK2_RC_FILES"\n', shell=shell,
                )
                self.assertEqual(result.stdout.strip(), str(gtk))
                for override in ("", "/explicit override"):
                    self.env.update(GTK2_RC_FILES=override, __GL_SHADER_DISK_CACHE_PATH=override)
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile-desktop.sh"\n'
                        'printf "%s\\n" "$GTK2_RC_FILES" "$__GL_SHADER_DISK_CACHE_PATH"\n',
                        shell=shell,
                    )
                    self.assertEqual(result.stdout.splitlines(), [override, override])
                self.env.pop("GTK2_RC_FILES")
                self.env.pop("__GL_SHADER_DISK_CACHE_PATH")
                gtk.unlink()

    def test_github_key_generation_stops_on_prerequisite_failures(self):
        setup = (
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ssh-keygen-github\n'
            'ssh-keygen() { print -r -- "unexpected key generation"; return 0; }\n'
        )
        for git, mkdir, expected in (
            ('return 19', 'return 0', 1),
            ('print -r -- "   "', 'return 0', 1),
            ('print -r -- fixture@test.local', 'return 23', 23),
        ):
            with self.subTest(git=git, mkdir=mkdir):
                result = self.run_shell(
                    setup + f'git() {{ {git}; }}\nmkdir() {{ {mkdir}; }}\nssh-keygen-github\n',
                    expected=expected,
                )
                self.assertEqual(result.stdout, "")
                self.assertFalse((self.home / ".ssh").exists())

    def test_github_key_generation_preserves_existing_key_paths(self):
        directory = self.home / ".ssh/github"
        directory.mkdir(parents=True)
        for filename in ("id_ed25519", "id_ed25519.pub"):
            for kind in ("file", "directory", "dangling-link"):
                with self.subTest(filename=filename, kind=kind):
                    path = directory / filename
                    if kind == "file":
                        path.write_text("existing key fixture\n")
                    elif kind == "directory":
                        path.mkdir()
                    else:
                        path.symlink_to(directory / "missing")
                    result = self.run_shell(
                        'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
                        'autoload -Uz ssh-keygen-github\n'
                        'git() { print -r -- "unexpected Git call"; }\n'
                        'ssh-keygen() { print -r -- "unexpected key generation"; }\n'
                        'ssh-keygen-github\n', expected=1,
                    )
                    self.assertEqual(result.stdout, "")
                    self.assertIn("Key already exists", result.stderr)
                    if kind == "file":
                        self.assertEqual(path.read_text(), "existing key fixture\n")
                    elif kind == "dangling-link":
                        self.assertTrue(path.is_symlink())
                    if kind == "directory":
                        path.rmdir()
                    else:
                        path.unlink()

    def test_github_key_generation_arguments_and_exit_status(self):
        result = self.run_shell(
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ssh-keygen-github\n'
            'git() { [[ "$*" == "config --global user.email" ]] || return 1; '
            'print -r -- fixture@test.local; }\n'
            'ssh-keygen() { printf "%s\\n" "$@"; return 29; }\n'
            'ssh-keygen-github\n', expected=29,
        )
        self.assertEqual(result.stdout.splitlines(), [
            "-t", "ed25519", "-C", "fixture@test.local", "-f",
            str(self.home / ".ssh/github/id_ed25519"),
        ])
        self.assertEqual((self.home / ".ssh/github").stat().st_mode & 0o777, 0o700)

    def test_ssh_helpers_secure_existing_directories(self):
        ssh = self.home / ".ssh"
        github = ssh / "github"
        github.mkdir(parents=True)
        for helper in ("ssh-keygen-github", "ssh-wsl"):
            with self.subTest(helper=helper):
                ssh.chmod(0o777)
                github.chmod(0o777)
                self.run_shell(
                    'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
                    f'autoload -Uz {helper}\n'
                    'git() { print -r -- fixture@test.local; }\n'
                    'ssh-keygen() { return 0; }\n'
                    'wslpath() { return 0; }\n'
                    'WINHOME="$HOME/windows"\n'
                    f'{helper}\n'
                )
                self.assertEqual(ssh.stat().st_mode & 0o777, 0o700)
                if helper == "ssh-keygen-github":
                    self.assertEqual(github.stat().st_mode & 0o777, 0o700)

    def test_ssh_helpers_do_not_chmod_linked_directories(self):
        outside = self.fixture / "linked-directory"
        outside.mkdir(mode=0o755)
        for helper, link in (("ssh-keygen-github", ".ssh"),
                             ("ssh-keygen-github", ".ssh/github"),
                             ("ssh-wsl", ".ssh")):
            with self.subTest(helper=helper, link=link):
                target = self.home / link
                target.parent.mkdir(parents=True, exist_ok=True)
                target.symlink_to(outside, target_is_directory=True)
                try:
                    result = self.run_shell(
                        'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
                        f'autoload -Uz {helper}\n'
                        'git() { print -r -- fixture@test.local; }\n'
                        'ssh-keygen() { print -r -- "unexpected key generation"; }\n'
                        'wslpath() { return 0; }\n'
                        'WINHOME="$HOME/windows"\n'
                        f'{helper}\n', expected=1,
                    )
                    self.assertIn("SSH directory symlink", result.stderr)
                    self.assertEqual(result.stdout, "")
                    self.assertEqual(outside.stat().st_mode & 0o777, 0o755)
                finally:
                    target.unlink()
                    if link == ".ssh/github":
                        target.parent.rmdir()

    def test_ssh_permission_failure_stops_key_generation(self):
        result = self.run_shell(
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ssh-keygen-github\n'
            'git() { print -r -- fixture@test.local; }\n'
            'chmod() { return 23; }\n'
            'ssh-keygen() { print -r -- "unexpected key generation"; }\n'
            'ssh-keygen-github\n', expected=23,
        )
        self.assertEqual(result.stdout, "")

    def test_bash_prompt_uses_one_git_lookup_and_suppresses_failures(self):
        calls = self.fixture / "prompt-git-calls"
        self.env["PROMPT_GIT_CALLS"] = str(calls)
        for status, output, expected in ((0, "main", " main"), (128, "partial", "")):
            with self.subTest(status=status):
                calls.unlink(missing_ok=True)
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/bash/prompt.bash"\n'
                    'git() { printf "%s\\n" "$*" >> "$PROMPT_GIT_CALLS"; '
                    f'printf %s {output}; return {status}; }}\n'
                    '__bash_git_prompt\n', shell="bash", expected=status,
                )
                self.assertEqual(result.stdout, expected)
                self.assertEqual(calls.read_text().splitlines(), ["rev-parse --abbrev-ref HEAD"])

    def test_sudo_widget_preserves_command_and_adjusts_cursor(self):
        for buffer in ("echo fixture", "sudo echo fixture"):
            for cursor in (0, 2, 5, 8, len(buffer)):
                with self.subTest(buffer=buffer, cursor=cursor):
                    output = self.fixture / "widget-result"
                    self.env.update(TEST_BUFFER=buffer, TEST_CURSOR=str(cursor), RESULT=str(output))
                    master, slave = pty.openpty()
                    process = subprocess.Popen(
                        ["zsh", "-dfi", "-c",
                         'source "$ROOT/symlinks/config/zsh/key-bindings.zsh"\n'
                         'probe() { BUFFER=$TEST_BUFFER; CURSOR=$TEST_CURSOR; '
                         'sudo-command-line; printf "%s\\n" "$BUFFER" "$CURSOR" > "$RESULT"; exit; }\n'
                         'zle -N probe; bindkey "^G" probe; vared -c -p "READY>" fixture_input'],
                        stdin=slave, stdout=slave, stderr=slave, env=self.env, cwd=self.fixture,
                    )
                    os.close(slave)
                    try:
                        received = b""
                        deadline = time.monotonic() + 5
                        while b"READY>" not in received:
                            self.assertLess(time.monotonic(), deadline, "ZLE did not start")
                            if select.select([master], [], [], 0.1)[0]:
                                try:
                                    received += os.read(master, 4096)
                                except OSError:
                                    self.fail(f"ZLE exited before starting: {received!r}")
                        os.write(master, b"\x07")
                        self.assertEqual(process.wait(timeout=5), 0)
                    finally:
                        if process.poll() is None:
                            process.kill()
                            process.wait()
                        os.close(master)
                    removing = buffer.startswith("sudo ")
                    self.assertEqual(output.read_text().splitlines(), [
                        buffer[5:] if removing else "sudo " + buffer,
                        str(max(0, cursor - 5) if removing else cursor + 5),
                    ])

    def test_mirror_update_preserves_destination_on_failures(self):
        destination = self.write(self.fixture / "pacman.d/mirrorlist", "original\n")
        helper = (ROOT / "symlinks/config/zsh/functions/update-mirrors").read_text()
        # Exercise the privileged staging script unprivileged against a fixture.
        helper = helper.replace("/etc/pacman.d/", str(destination.parent) + "/")
        self.write(self.fixture / "update-mirrors", helper)
        self.env["PATH"] = str(self.fixture / "bin") + ":/usr/bin:/bin"
        self.env["TMPDIR"] = str(self.fixture / "tmp")
        Path(self.env["TMPDIR"]).mkdir()
        setup = ('sudo() { "$@"; }\n'
                 'fpath=("$PWD" $fpath)\nautoload -Uz update-mirrors\n')
        for failure in ("generation", "empty", "invalid", "copy", "rename", "none"):
            with self.subTest(failure=failure):
                destination.write_text("original\n")
                self.write(self.fixture / "bin/reflector", "#!/bin/sh\n" + {
                    "generation": "printf 'partial\n'; exit 19\n",
                    "empty": "printf '# no servers\n'\n",
                    "invalid": "printf 'Server = http://insecure.example/$repo/os/$arch\n'\n",
                }.get(failure, "printf 'Server = https://mirror.example/$repo/os/$arch\n'\n"),
                    executable=True)
                for command, body in (
                    ("install", 'printf partial > "$5"; exit 23\n' if failure == "copy"
                     else 'exec /usr/bin/install "$@"\n'),
                    ("mv", 'exit 29\n' if failure == "rename" else 'exec /usr/bin/mv "$@"\n'),
                ):
                    self.write(self.fixture / "bin" / command, "#!/bin/sh\n" + body, executable=True)
                expected = {"generation": 19, "empty": 1, "invalid": 1,
                            "copy": 23, "rename": 29, "none": 0}[failure]
                self.run_shell(setup + "update-mirrors\n", expected=expected)
                self.assertEqual(destination.read_text(),
                                 "Server = https://mirror.example/$repo/os/$arch\n"
                                 if failure == "none" else "original\n")
                self.assertEqual(list(destination.parent.iterdir()), [destination])
                self.assertEqual(list(Path(self.env["TMPDIR"]).iterdir()), [])
                if failure == "none":
                    self.assertEqual(destination.stat().st_mode & 0o777, 0o644)

    def test_xdg_directory_service_matches_config_and_preserves_content(self):
        unit = configparser.ConfigParser(interpolation=None)
        unit.read(ROOT / "symlinks/config/systemd/user/xdg-user-directories.service")
        command = shlex.split(unit["Service"]["ExecStart"])
        command = [part.replace("%h", str(self.home)) for part in command]
        configured = (ROOT / "symlinks/config/user-dirs.dirs").read_text().splitlines()
        directories = {Path(line.split("=", 1)[1].strip('"').replace("$HOME", str(self.home)))
                       for line in configured} - {self.home}
        self.assertEqual({Path(part) for part in command[3:]}, directories)
        preserved = self.write(self.home / "media/documents/existing", "keep\n")
        for _ in range(2):
            subprocess.run(command, env=self.env, check=True, timeout=10)
            self.assertTrue(all(directory.is_dir() for directory in directories))
            self.assertEqual(preserved.read_text(), "keep\n")
        conflict = self.home / "templates"
        conflict.rmdir()
        conflict.write_text("user file\n")
        result = subprocess.run(command, env=self.env, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(conflict.read_text(), "user file\n")

    def test_pacman_alias_does_not_override_host_configuration(self):
        self.assertEqual(
            (ROOT / "symlinks/config/pacman.conf").read_text().splitlines()[-1],
            "Include = /etc/pacman.conf",
        )
        self.assertNotIn(
            '"config/pacman.conf"', (ROOT / "conf/symlinks.toml").read_text(),
        )
        self.write(self.home / ".config/pacman.conf", "[options]\nColor\n")
        binary = self.write(
            self.fixture / "bin/pacman", '#!/bin/sh\nprintf "%s\\n" "$@"\n',
            executable=True,
        )
        self.env["PATH"] = str(binary.parent) + ":/usr/bin:/bin"
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/aliases.sh"\n'
                    'if alias pacman >/dev/null 2>&1; then exit 1; fi\n'
                    'pacman -Q\n', shell=shell,
                )
                self.assertEqual(result.stdout.strip(), "-Q")

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
                'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
                '[[ ${_comps[dotfiles]} == _clap_dynamic_completer_dotfiles ]]\n'
            )

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
            '[[ ${_comps[dotfiles]} == _clap_dynamic_completer_dotfiles ]]\n'
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
        for source, contents, hostname in (
            ("config", "Host new-config-alias\n  HostName example.test\n", "new-config-alias"),
            ("known_hosts", "new-known-host ssh-ed25519 fixture\n", "new-known-host"),
        ):
            with self.subTest(source=source):
                if "ZSH_COMPDUMP" not in self.env:
                    self.prepare_completion()
                cache = self.home / ".cache/zsh/hosts.cache"
                self.write(cache, "typeset -ga hosts=(localhost)\n")
                changed = self.write(self.home / ".ssh" / source, contents)
                stamp = int(time.time()) - 20
                os.utime(cache, (stamp, stamp))
                os.utime(changed, (stamp + 2, stamp + 2))
                script = (
                    'setopt extendedglob\n'
                    'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
                    'print -rl -- "${hosts[@]}"\n'
                )
                result = self.run_shell(script)
                self.assertIn(hostname, result.stdout.splitlines())
                updated = cache.stat().st_mtime_ns
                self.run_shell(script)
                self.assertEqual(cache.stat().st_mtime_ns, updated,
                                 "unchanged sources should reuse the refreshed cache")

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

    def write_rpm(self, members):
        payload = bytearray()
        for name, data in [*members, ("TRAILER!!!", b"")]:
            encoded_name = name.encode() + b"\0"
            fields = (
                1, 0o100600, 0, 0, 1, int(time.time()) + 3600, len(data),
                0, 0, 0, 0, len(encoded_name), 0,
            )
            entry = b"070701" + "".join(f"{value:08x}" for value in fields).encode()
            entry += encoded_name
            entry += b"\0" * (-len(entry) % 4)
            entry += data + b"\0" * (-len(data) % 4)
            payload.extend(entry)
        # Minimal RPM lead and empty signature/header sections around a cpio payload.
        lead = struct.pack(
            ">4sBBHH66sHH16s", b"\xed\xab\xee\xdb", 3, 0, 0, 1,
            b"synthetic-security-fixture", 1, 5, b"",
        )
        header = struct.pack(">4s4sII", b"\x8e\xad\xe8\x01", b"", 0, 0)
        archive = self.fixture / "fixture with spaces.rpm"
        archive.write_bytes(lead + header + header + gzip.compress(payload))
        return archive

    def run_extract(self, archive, expected=0):
        self.env["ARCHIVE"] = str(archive)
        return self.run_shell(
            'cd "$HOME"\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz extract\n'
            'extract "$ARCHIVE"\n',
            expected=expected,
        )

    def test_extract_rpm_reads_payload(self):
        archive = self.write_rpm([("nested/file with spaces", b"fixture\n")])
        self.run_extract(archive)
        self.assertEqual((self.home / "nested/file with spaces").read_bytes(), b"fixture\n")

    def test_extract_option_like_relative_archive_name(self):
        archive = self.home / "-sample with spaces.gz"
        archive.write_bytes(gzip.compress(b"fixture\n"))
        self.run_extract(archive.name)
        self.assertEqual((self.home / "-sample with spaces").read_bytes(), b"fixture\n")

    def test_extract_rejects_multiple_archives_before_changing_any_files(self):
        archives = [self.home / name for name in ("first.gz", "second.gz")]
        payload = gzip.compress(b"fixture\n")
        for archive in archives:
            archive.write_bytes(payload)
        result = self.run_shell(
            'cd "$HOME"\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz extract\n'
            'extract first.gz second.gz\n', expected=1,
        )
        self.assertIn("Usage: extract", result.stdout)
        self.assertEqual(sorted(path.name for path in self.home.iterdir()),
                         ["first.gz", "second.gz"])
        for archive in archives:
            self.assertEqual(archive.read_bytes(), payload)

    def test_extract_rpm_confines_member_paths(self):
        target = self.write(self.fixture / "outside", "original\n")
        (self.home / "escape").symlink_to(self.fixture, target_is_directory=True)
        for member, expected in (
            ("../outside", 1),
            (str(target), 0),
            ("escape/outside", 1),
        ):
            with self.subTest(member=member):
                archive = self.write_rpm([(member, b"replaced\n")])
                result = self.run_extract(archive, expected=expected)
                self.assertEqual(target.read_text(), "original\n")
                self.assertTrue(result.stderr, "unsafe paths must not be handled silently")

    def test_extract_rpm_propagates_invalid_archive_failure(self):
        archive = self.write(self.fixture / "invalid.rpm", "not an archive\n")
        result = self.run_extract(archive, expected=1)
        self.assertIn("Error", result.stderr)
        self.assertEqual(list(self.home.iterdir()), [])

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
