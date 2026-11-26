"""SSH helper, prompt, widget, mirror, XDG, and pacman regression tests."""

import configparser
import os
from pathlib import Path
import pty
import re
import select
import shlex
import shutil
import subprocess
import time

from shell_config_tests.base import ROOT, ShellConfigTestCase


class SshPromptAndSystemTests(ShellConfigTestCase):
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

    def test_ssh_wsl_resolves_windows_profile_without_windows_path(self):
        windows = self.fixture / "Windows tools"
        profile = self.fixture / "Windows User"
        keys = profile / ".ssh/github"
        keys.mkdir(parents=True)
        self.write(windows / "cmd.exe",
                   "#!/bin/sh\n"
                   '[ "$3" = \'echo "%USERPROFILE%"\' ] || exit 2\n'
                   "printf '\"C:\\\\Users\\\\Windows User\"\\r\\n'\n",
                   executable=True)
        self.env["WINDOWS_TOOLS"] = str(windows)
        self.env["WINDOWS_PROFILE"] = str(profile)
        self.run_shell(
            'unset WINHOME WSL_CMD_EXE\n'
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ssh-wsl\n'
            'command -v cmd.exe >/dev/null && exit 2\n'
            'wslpath() {\n'
            '  case "$*" in\n'
            '    \'-u C:\\Windows\\System32\\cmd.exe\') print -r -- "$WINDOWS_TOOLS/cmd.exe";;\n'
            '    \'-a -u C:\\Users\\Windows User\') print -r -- "$WINDOWS_PROFILE";;\n'
            '    *) return 3;;\n'
            '  esac\n'
            '}\n'
            'ssh-wsl\n'
        )
        self.assertEqual((self.home / ".ssh/github").resolve(), keys)

    def test_ssh_wsl_rejects_bad_override_before_changing_ssh(self):
        relative = self.fixture / "Windows profile"
        (relative / ".ssh/github").mkdir(parents=True)
        ssh = self.home / ".ssh"
        ssh.mkdir()
        ssh.chmod(0o750)
        marker = self.write(ssh / "operator-note", "keep this SSH customization\n")
        for override in (relative.name, str(relative / "missing")):
            with self.subTest(override=override):
                self.env["WINHOME"] = override
                result = self.run_shell(
                    'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
                    'autoload -Uz ssh-wsl\nwslpath() { return 1; }\nssh-wsl\n',
                    expected=1,
                )
                self.assertIn("existing absolute", result.stderr)
                self.assertEqual(ssh.stat().st_mode & 0o777, 0o750)
                self.assertEqual(list(ssh.iterdir()), [marker])
        self.env["WINHOME"] = str(relative)
        self.run_shell(
            'fpath=("$ROOT/symlinks/config/zsh/functions" $fpath)\n'
            'autoload -Uz ssh-wsl\nwslpath() { return 1; }\nssh-wsl && ssh-wsl\n'
        )
        self.assertEqual((ssh / "github").resolve(strict=True), relative / ".ssh/github")
        self.assertEqual(marker.read_text(), "keep this SSH customization\n")

    def test_ssh_helpers_secure_existing_directories(self):
        ssh = self.home / ".ssh"
        github = ssh / "github"
        github.mkdir(parents=True)
        (self.home / "windows").mkdir()
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
        (self.home / "windows").mkdir()
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
                    'printf "git diagnostic\\n" >&2; '
                    f'printf %s {output}; return {status}; }}\n'
                    '__bash_git_prompt\n', shell="bash", expected=status,
                )
                self.assertEqual(result.stdout, expected)
                self.assertEqual(result.stderr, "")
                self.assertEqual(calls.read_text().splitlines(), ["rev-parse --abbrev-ref HEAD"])

    def test_zsh_prompt_renders_current_branch_in_one_lookup(self):
        result = self.run_shell(r'''
            set -e
            cd "$HOME"
            git init -q --template= -b main repo
            cd repo
            print seed >tracked
            git add tracked
            git -c user.name=Fixture -c user.email=fixture@test.local commit -qm seed
            source "$ROOT/symlinks/config/zsh/prompt.zsh"
            git() {
                print -r -- "$ZSH_SUBSHELL:$*" >> "$HOME/lookups"
                command git "$@"
            }
            render() { false || print -Pr -- "${PROMPT}%?"; }
            render
            command git switch -qc 'topic%F{red}-$HOME'
            render
            print changed >tracked
            render
            unfunction git
            PATH="$HOME/no-tools"
            print -Pr -- "${PROMPT}%?"
        ''')
        raw = result.stdout.splitlines()
        self.assertEqual(raw[2:4], raw[4:6], "dirtying a tracked file changed the prompt")
        lines = re.sub(r'\x1b\[[\d;]*m', '', result.stdout).splitlines()
        sigil = "#" if os.geteuid() == 0 else "$"
        self.assertEqual(lines[1::2], [f"{sigil} {status}" for status in (1, 1, 1, 0)])
        for line, branch in zip(lines[::2], (" main", " topic%F{red}-$HOME",
                                              " topic%F{red}-$HOME", "")):
            self.assertTrue(line.endswith("~/repo" + branch), line)
        self.assertEqual(result.stderr, "")
        # Only the branch segment and its Git lookup need command substitution.
        calls = (self.home / "lookups").read_text().splitlines()
        self.assertEqual(len(calls), 3)
        for call in calls:
            depth, arguments = call.split(":", 1)
            self.assertLessEqual(int(depth), 2)
            self.assertEqual(arguments, "rev-parse --abbrev-ref HEAD")

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
                self.write(self.fixture / "bin/reflector",
                           "#!/bin/sh\n"
                           '[ "$1" = @/etc/xdg/reflector/reflector.conf ] && '
                           '[ "$2" = --save ] && [ "$#" = 3 ] || exit 2\n'
                           'exec > "$3"\n' + {
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
        if shutil.which("xdg-user-dir"):
            # Older xdg-user-dir shell scripts (xdg-user-dirs 0.18) do not quote
            # $XDG_CONFIG_HOME or $HOME when sourcing user-dirs.dirs.
            xdg_home = self.fixture / "xdg-home"
            config_dir = xdg_home / ".config"
            config_dir.mkdir(parents=True, exist_ok=True)
            (config_dir / "user-dirs.dirs").symlink_to(ROOT / "symlinks/config/user-dirs.dirs")
            resolved = subprocess.run(
                ["xdg-user-dir", "DESKTOP"],
                env={**self.env, "HOME": str(xdg_home), "XDG_CONFIG_HOME": str(config_dir)},
                check=True, text=True, capture_output=True, timeout=10,
            )
            self.assertEqual(Path(resolved.stdout.strip()), xdg_home)
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
        self.assertFalse((ROOT / "symlinks/config/pacman.conf").exists())
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
