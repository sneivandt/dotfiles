"""Login profile, PATH, environment, history, and keymap regression tests."""

import json
import os
from pathlib import Path
import pty
import shutil
import subprocess

from shell_config_tests.base import ROOT, ShellConfigTestCase


class ProfileAndEnvTests(ShellConfigTestCase):
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

    def test_installed_bash_login_keeps_the_users_previously_selected_profile(self):
        profile = self.write(
            self.home / ".profile",
            'export EDITOR="site-editor"\nexport PATH="$HOME/site tools:$PATH"\n'
            'printf "profile\\n" >> "$HOME/load-order"\n',
        )
        login = self.home / ".bash_login"
        site_login = self.fixture / "site policies/bash-login"
        login.symlink_to(site_login)  # unavailable until the site's profile is restored
        entry = self.home / ".bash_profile"
        entry.symlink_to(ROOT / "symlinks/bash_profile")
        # A controlled PATH prevents optional host integrations from being loaded.
        self.install_profile()
        script = (
            'PATH="$HOME/empty-bin"\ntty() { return 1; }\n'
            '. "$HOME/.bash_profile"\n. "$HOME/.bash_profile"\n'
            'printf "%s\\n%s\\n" "$EDITOR" "$PATH"\n'
        )
        editor, path = self.run_shell(script, shell="bash").stdout.splitlines()
        self.assertEqual(editor, "site-editor")
        self.assertEqual(path.split(":").count(str(self.home / "site tools")), 1)
        self.assertEqual((self.home / "load-order").read_text(), "profile\n")
        # Bash prefers .bash_login over .profile. Do not double-load both.
        self.write(site_login, 'export EDITOR="login-editor"\nprintf "bash_login\\n" >> "$HOME/load-order"\n')
        editor, path = self.run_shell(script, shell="bash").stdout.splitlines()
        self.assertEqual(editor, "login-editor")
        self.assertNotIn(str(self.home / "site tools"), path.split(":"))
        self.assertEqual((self.home / "load-order").read_text(), "profile\nbash_login\n")
        self.assertIn('export EDITOR="site-editor"', profile.read_text())

    def test_installed_bash_login_does_not_duplicate_unmanaged_interactive_startup(self):
        (self.home / ".bash_profile").symlink_to(ROOT / "symlinks/bash_profile")
        rc = self.write(self.home / ".bashrc", 'printf "rc\\n" >> "$HOME/load-order"\n')
        profile = self.write(self.home / ".profile", '. "$HOME/.bashrc"\n')
        script = '. "$HOME/.bash_profile"\n. "$HOME/.bash_profile"\n'
        self.run_shell(script, shell="bash", interactive=True)
        log = self.home / "load-order"
        self.assertEqual(log.read_text(), "rc\n")
        # A profile with only a PATH export should still get the real managed
        # rc. Hide vendor integrations; don't load machine startup files.
        profile.write_text('export PATH="$HOME/site tools:$PATH"\n')
        self.install_profile()
        rc.unlink()
        rc.symlink_to(ROOT / "symlinks/bashrc")
        self.write(self.home / ".cache/bash/history", "record-from-previous-login\n")
        # Keep _dotfiles_init_history callable while blocking optional external tools.
        prefix = (
            'tty() { return 1; }\n'
            'function [ { case "$*" in *"/usr/share/"*) return 1;; esac; builtin [ "$@"; }\n'
            'command() {\n'
            '  if [[ $1 == -v && $2 != _dotfiles_init_history ]]; then return 1; fi\n'
            '  builtin command "$@"\n'
            '}\n'
        )
        self.run_shell(prefix + script, shell="bash", interactive=True)
        history = self.home / ".local/state/bash/history"
        self.assertIn("record-from-previous-login\n", history.read_text())
        self.assertEqual(log.read_text(), "rc\n")

    def test_interactive_login_preserves_user_nounset(self):
        managed = ROOT / "symlinks"
        (self.home / ".config/shell").mkdir(parents=True)
        for source in (managed / "config/shell").iterdir():
            if source.name == "interactive.sh":
                self.write(
                    self.home / ".config/shell/interactive.sh",
                    source.read_text().replace("/usr/share/", '"$HOME"/no-packages/'),
                )
            else:
                (self.home / ".config/shell" / source.name).symlink_to(source)
        for directory in ("bash", "zsh"):
            (self.home / ".config" / directory).symlink_to(managed / "config" / directory)
        # A retained Bash .profile and a Zsh user's .zshenv run before the managed rc.
        self.write(self.home / ".profile", "set -u\n")
        self.write(self.home / ".zshenv",
                   "set -u\nfpath=(${^fpath}/compinit(N:h) ${^fpath}/select-word-style(N:h))\n")
        # Only core utilities, no optional init commands or system plugin files.
        bindir = self.fixture / "bin"
        bindir.mkdir()
        for name in ("bash", "zsh", "tty", "dirname", "mkdir", "readlink", "touch", "mktemp", "mv"):
            (bindir / name).symlink_to(shutil.which(name, path=self.env["PATH"]))
        self.env["PATH"] = str(bindir)
        self.env["TERM"] = "dumb"
        del self.env["SHELL"]
        for shell, profile, helper in (("bash", "bash_profile", "__bash_git_prompt"),
                                       ("zsh", "zprofile", "git_prompt_info")):
            with self.subTest(shell=shell):
                (self.home / ("." + profile)).symlink_to(managed / profile)
                rc = shell + "rc"
                self.write(self.home / ("." + rc), (managed / rc).read_text().replace(
                    "/usr/share/", '"$HOME"/no-packages/'))
                # Source the Bash login entry explicitly to skip the host's /etc/profile.
                args = (["bash", "--noprofile", "--norc", "-lic"] if shell == "bash"
                        else ["zsh", "-dlic"])
                ready = (f'[[ -o nounset && $EDITOR == vim ]] && alias dot >/dev/null && '
                         f'typeset -f {helper} >/dev/null')
                script = ('. ~/.bash_profile\n' if shell == "bash" else "") + (
                    f'{ready} || exit 1\n. ~/.{profile}\n. ~/.{rc}\n{ready}')
                result = subprocess.run(args + [script], cwd=self.fixture, env=self.env,
                                        text=True, capture_output=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotRegex(result.stderr, "unbound variable|parameter not set")

    def test_uwsm_needs_arch_desktop_and_falls_back_to_shell(self):
        self.install_profile()
        self.write(
            self.home / ".local/bin/uwsm",
            "#!/bin/sh\n"
            'case "$1" in check) exit 0;; start) printf "%s\\n" "$PATH"; exit "${UWSM_STATUS:-0}";; esac\n',
            executable=True,
        )
        script = (
            'f() {\n'
            '  local -h TTY=""\n'
            '  tty() { [ "$1" = -s ] || printf "/dev/tty1\\n"; }\n'
            '  source "$ROOT/symlinks/zprofile"\n'
            '}\n'
            'f\n'
            'echo shell-ready\n'
        )
        master, slave = pty.openpty()
        try:
            def run_interactive_tty():
                result = subprocess.run(
                    ["zsh", "-dfi", "-c", script], cwd=self.fixture, env=self.env,
                    stdin=slave, text=True, capture_output=True, timeout=20,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                return result

            self.assertEqual(run_interactive_tty().stdout, "shell-ready\n")
            (self.home / ".config/shell/profile-arch-desktop.sh").symlink_to(
                ROOT / "symlinks/config/shell/profile-arch-desktop.sh"
            )
            for status in ("0", "42"):
                with self.subTest(status=status):
                    self.env["UWSM_STATUS"] = status
                    result = run_interactive_tty()
                    path, shell = result.stdout.splitlines()
                    self.assertIn(str(self.home / ".local/bin"), path.split(":"))
                    self.assertEqual(shell, "shell-ready")
                    self.assertEqual("Could not start Hyprland" in result.stderr, status != "0")
        finally:
            os.close(master)
            os.close(slave)

    def test_uwsm_skips_non_interactive_or_redirected_stdin_on_tty1(self):
        self.install_profile()
        (self.home / ".config/shell/profile-arch-desktop.sh").symlink_to(
            ROOT / "symlinks/config/shell/profile-arch-desktop.sh"
        )
        marker = self.fixture / "uwsm-invoked"
        self.env["UWSM_MARKER"] = str(marker)
        self.write(
            self.home / ".local/bin/uwsm",
            '#!/bin/sh\nprintf invoked > "$UWSM_MARKER"\nexit 99\n',
            executable=True,
        )
        self.write(
            self.home / ".local/bin/tty",
            '#!/bin/sh\n[ "${1:-}" = -s ] || printf "/dev/tty1\\n"\n',
            executable=True,
        )
        (self.home / ".zprofile").symlink_to(ROOT / "symlinks/zprofile")
        tty_set_script = (
            'f() {\n'
            '  local -h TTY=/dev/tty1\n'
            '  source "$ROOT/symlinks/zprofile"\n'
            '}\n'
            'f\n'
            'echo shell-ready\n'
        )
        tty_empty_script = (
            'f() {\n'
            '  local -h TTY=""\n'
            '  source "$ROOT/symlinks/zprofile"\n'
            '}\n'
            'f\n'
            'echo shell-ready\n'
        )
        master, slave = pty.openpty()
        try:
            cases = (
                ("non-interactive-tty-set", ["zsh", "-df", "-c", tty_set_script], slave),
                ("redirected-stdin-tty-set", ["zsh", "-dfi", "-c", tty_set_script], subprocess.DEVNULL),
                ("non-interactive-login-empty-tty", ["zsh", "-dl", "-c", "echo shell-ready"], slave),
                ("non-interactive-empty-tty", ["zsh", "-df", "-c", tty_empty_script], slave),
                ("redirected-stdin-empty-tty", ["zsh", "-dfi", "-c", tty_empty_script], subprocess.DEVNULL),
            )
            env = {**self.env, "ZDOTDIR": str(self.home), "TTY": ""}
            for label, args, stdin in cases:
                with self.subTest(case=label):
                    if marker.exists():
                        marker.unlink()
                    result = subprocess.run(
                        args, cwd=self.fixture, env=env,
                        stdin=stdin, text=True, capture_output=True, timeout=20,
                    )
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    self.assertEqual(result.stdout, "shell-ready\n")
                    self.assertFalse(marker.exists(), f"{label} must not invoke uwsm")
        finally:
            os.close(master)
            os.close(slave)

    def test_path_finds_cargo_commands_in_custom_cargo_home(self):
        cargo_home = self.fixture / "custom cargo home"
        self.write(cargo_home / "bin/cargo-fixture", '#!/bin/sh\nprintf custom-cargo', executable=True)
        self.env["CARGO_HOME"] = str(cargo_home)
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/path.sh"\n'
                    'cargo-fixture\n', shell=shell,
                )
                self.assertEqual(result.stdout, "custom-cargo")

    def test_manpager_falls_back_when_col_is_unavailable(self):
        for shell in ("bash", "zsh"):
            for col_available in (True, False):
                with self.subTest(shell=shell, col_available=col_available):
                    self.env["COL_AVAILABLE"] = "0" if col_available else "1"
                    result = self.run_shell(
                        'command() {\n'
                        '  if [ "$1" = -v ]; then\n'
                        '    case "$2" in\n'
                        '      bat) return 0;;\n'
                        '      col) return "$COL_AVAILABLE";;\n'
                        '    esac\n'
                        '  fi\n'
                        '  builtin command "$@"\n'
                        '}\n'
                        '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                        'printf "%s\\n" "$MANPAGER"\n', shell=shell,
                    )
                    if col_available:
                        self.assertIn("col -bx | bat", result.stdout)
                    else:
                        self.assertEqual(result.stdout, "less\n")

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

    def test_profile_preserves_explicit_editor_and_visual_choices(self):
        for shell in ("bash", "zsh"):
            for editor, visual in (("nano", "code --wait"), ("nano", None)):
                with self.subTest(shell=shell, editor=editor, visual=visual):
                    self.env["EDITOR"] = editor
                    self.env.pop("VISUAL", None)
                    if visual is not None:
                        self.env["VISUAL"] = visual
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                        'printf "%s\\n" "$EDITOR" "$VISUAL"\n', shell=shell,
                    )
                    self.assertEqual(result.stdout.splitlines(), [editor, visual or editor])

    def test_profile_uses_custom_go_workspace_and_install_directory(self):
        self.install_profile()
        workspace = self.fixture / "go workspace"
        custom_bin = self.fixture / "go binaries"
        self.env["GOPATH"] = str(workspace) + ":" + str(self.fixture / "second workspace")
        for shell in ("bash", "zsh"):
            for gobin in (None, str(custom_bin)):
                with self.subTest(shell=shell, gobin=gobin):
                    self.env.pop("GOBIN", None)
                    if gobin is not None:
                        self.env["GOBIN"] = gobin
                    directory = Path(gobin) if gobin else workspace / "bin"
                    self.write(directory / "go-fixture", '#!/bin/sh\nprintf custom-go', executable=True)
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                        'printf "%s\\n" "$GOPATH"\n'
                        'go-fixture\n', shell=shell,
                    )
                    self.assertEqual(result.stdout, self.env["GOPATH"] + "\ncustom-go")

    def test_profile_preserves_explicit_manpager(self):
        self.env["MANPAGER"] = "fixture-pager --raw"
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                    'printf "%s\\n" "$MANPAGER"\n', shell=shell,
                )
                self.assertEqual(result.stdout, "fixture-pager --raw\n")

    def test_profile_preserves_fzf_search_commands_and_widget_options(self):
        preferences = {
            "FZF_DEFAULT_COMMAND": "find . -type f",
            "FZF_CTRL_T_COMMAND": "find . -name '*.txt'",
            "FZF_ALT_C_COMMAND": "find . -type d",
            "FZF_DEFAULT_OPTS": "--layout=default",
            "FZF_CTRL_T_OPTS": "--preview 'head -40 -- {}'",
            "FZF_ALT_C_OPTS": "",  # An explicitly disabled preview stays disabled.
            "FZF_CTRL_R_OPTS": "--exact",
        }
        self.env.update(preferences)
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    'fd() { :; }\n'
                    '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                    'python3 -c \'import json, os; print(json.dumps({key: value for key, value in os.environ.items() if key.startswith("FZF_")}))\'\n',
                    shell=shell,
                )
                self.assertEqual(json.loads(result.stdout), preferences)

    def test_history_search_defaults_do_not_require_fd(self):
        for shell in ("bash", "zsh"):
            with self.subTest(shell=shell):
                result = self.run_shell(
                    'command() {\n'
                    '  [ "$1" != -v ] || [ "$2" != fd ] || return 1\n'
                    '  builtin command "$@"\n'
                    '}\n'
                    '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                    'printf "%s\\n" "${FZF_CTRL_R_OPTS-unset}"\n', shell=shell,
                )
                self.assertEqual(result.stdout, "--no-preview\n")

    def test_qt_backend_respects_overrides_and_session(self):
        for shell in ("bash", "zsh"):
            for backend, expected in (
                ("offscreen", "offscreen"),
                ("xcb", "xcb"),
                (None, "wayland;xcb"),
            ):
                with self.subTest(shell=shell, source="uwsm", backend=backend):
                    self.env.pop("QT_QPA_PLATFORM", None)
                    if backend is not None:
                        self.env["QT_QPA_PLATFORM"] = backend
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/uwsm/env-hyprland.d/session.sh"\n'
                        'printf "%s\\n" "${QT_QPA_PLATFORM-unset}"\n', shell=shell,
                    )
                    self.assertEqual(result.stdout.strip(), expected)
            with self.subTest(shell=shell, source="profile-arch-desktop"):
                self.env.pop("QT_QPA_PLATFORM", None)
                self.env.pop("MOZ_ENABLE_WAYLAND", None)
                result = self.run_shell(
                    '. "$ROOT/symlinks/config/shell/profile-arch-desktop.sh"\n'
                    'printf "%s\\n%s\\n" "${QT_QPA_PLATFORM-unset}" "${MOZ_ENABLE_WAYLAND-unset}"\n',
                    shell=shell,
                )
                self.assertEqual(result.stdout.splitlines(), ["unset", "1"])

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

    def test_history_migration_never_publishes_a_partial_copy(self):
        body = "# recognizable history\noperator-kept-first\noperator-kept-last\n"
        for shell, label in (("bash", "Bash"), ("zsh", "Zsh")):
            with self.subTest(shell=shell):
                old = self.write(self.home / f".cache/{shell}/history", body)
                new = self.home / f".local/state/{shell}/history"
                script = (
                    '. "$ROOT/symlinks/config/shell/interactive.sh"\n'
                    f'_dotfiles_init_history {shell} {label}\n'
                )
                report = 'printf "%s\\n" "$HISTFILE"\n'
                failed = self.run_shell(
                    'cp() { printf interrupted > "$3"; return 1; }\n' + script + report,
                    shell=shell,
                )
                self.assertEqual(failed.stdout.rstrip(), str(old))
                self.assertEqual(list(new.parent.iterdir()), [])
                self.assertEqual(old.read_text(), body)
                retry = self.run_shell(script + report, shell=shell)
                self.assertEqual(retry.stdout.rstrip(), str(new))
                self.assertEqual(new.read_text(), body)
                self.assertEqual(new.stat().st_mode & 0o777, 0o600)
                new.unlink()
                self.run_shell(
                    'ln() { printf parallel-login > "$4"; return 1; }\n' + script + report,
                    shell=shell,
                )
                self.assertEqual(new.read_text(), "parallel-login")
                self.run_shell(script + report, shell=shell)
                self.assertEqual(new.read_text(), "parallel-login")
                self.assertEqual(old.read_text(), body)

    def test_history_retains_duplicate_expiration_headroom(self):
        result = self.run_shell(
            'source "$ROOT/symlinks/zshrc"\n'
            '[[ -o hist_expire_dups_first ]] && (( HISTSIZE > SAVEHIST ))\n'
            'print -r -- "$SAVEHIST"\n', interactive=True,
        )
        self.assertEqual(result.stdout.strip(), "100000")

    def test_profile_preserves_environment_preferences_and_defaults(self):
        variables = ("XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME",
                     "XDG_STATE_HOME", "TERMINFO", "TERMINFO_DIRS", "LESS")
        supplied = [str(self.fixture / name) for name in variables[:-1]] + ["-FR"]
        defaults = [str(self.home / suffix) for suffix in
                    (".cache", ".config", ".local/share", ".local/state", ".local/share/terminfo")]
        defaults += [str(self.home / ".local/share/terminfo") + ":", "-imRj8"]
        for shell in ("bash", "zsh"):
            for values in (supplied, defaults):
                with self.subTest(shell=shell, values=values):
                    for variable in variables:
                        self.env.pop(variable, None)
                    if values is supplied:
                        self.env.update(zip(variables, supplied))
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                        'printf "%s\\n" "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME" '
                        '"$XDG_DATA_HOME" "$XDG_STATE_HOME" "$TERMINFO" "$TERMINFO_DIRS" "$LESS"\n',
                        shell=shell,
                    )
                    self.assertEqual(result.stdout.splitlines(), values)
            self.env["LESS"] = ""
            self.run_shell(
                '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                '[ "${LESS+x}" = x ] && [ -z "$LESS" ]\n', shell=shell,
            )

    def test_custom_xdg_layout_warns_without_breaking_wget(self):
        self.env["XDG_CONFIG_HOME"] = str(self.home / "custom config")
        for installed in (False, True):
            if installed:
                self.write(self.home / ".config/wgetrc", "check_certificate = on\n")
                self.write(self.home / ".config/readline/inputrc", "set editing-mode vi\n")
            for shell in ("bash", "zsh"):
                with self.subTest(installed=installed, shell=shell):
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                        'printf "%s\\n" "${WGETRC-unset}" "${INPUTRC-unset}"\n'
                        'wget --version >/dev/null\n', shell=shell,
                    )
                    self.assertIn("custom XDG_CONFIG_HOME is unsupported", result.stderr)
                    self.assertEqual(result.stdout.splitlines(), [
                        str(self.home / ".config/wgetrc") if installed else "unset",
                        str(self.home / ".config/readline/inputrc") if installed else "unset",
                    ])
        self.assertFalse(Path(self.env["XDG_CONFIG_HOME"]).exists())

    def test_profile_preserves_readline_and_wget_overrides(self):
        custom = self.home / "custom config"
        self.env["XDG_CONFIG_HOME"] = str(custom)
        for suffix in ("wgetrc", "readline/inputrc"):
            self.write(custom / suffix, "# fixture\n")
        for explicit in (False, True):
            if explicit:
                self.env.update(WGETRC=str(custom / "chosen-wgetrc"),
                                INPUTRC=str(custom / "chosen-inputrc"))
            for shell in ("bash", "zsh"):
                with self.subTest(explicit=explicit, shell=shell):
                    result = self.run_shell(
                        '. "$ROOT/symlinks/config/shell/profile.sh"\n'
                        'printf "%s\\n" "$WGETRC" "$INPUTRC"\n', shell=shell,
                    )
                    self.assertEqual(result.stdout.splitlines(), [
                        self.env["WGETRC"] if explicit else str(custom / "wgetrc"),
                        self.env["INPUTRC"] if explicit else str(custom / "readline/inputrc"),
                    ])

    def test_approximate_completion_requires_repeated_unchanged_input(self):
        self.prepare_completion()
        result = self.run_shell(
            'setopt extendedglob\n'
            'source "$ROOT/symlinks/config/zsh/completion.zsh" || exit $?\n'
            'HISTNO=1 BUFFER="cat typo" CURSOR=8\n'
            'for step in first repeat edit move history; do\n'
            '  case $step in\n'
            '    edit) BUFFER="cat other" ;;\n'
            '    move) CURSOR=3 ;;\n'
            '    history) HISTNO=2 ;;\n'
            '  esac\n'
            '  zstyle -a ":completion::complete:cat:argument-1:" completer selected || exit 1\n'
            '  print -r -- "$step:${(j: :)selected}"\n'
            'done\n'
        )
        base = "_expand _complete _ignored"
        self.assertEqual(result.stdout.splitlines(),
                         ["first:" + base, "repeat:" + base + " _approximate",
                          "edit:" + base, "move:" + base, "history:" + base])

    def test_git_search_and_contributors_work_with_redirected_io(self):
        command = ["git", "-c", "init.templateDir=", "-c", "core.hooksPath=/dev/null",
                   "-c", "include.path=" + str(ROOT / "symlinks/config/git/aliases"),
                   "-c", "user.name=Fixture", "-c", "user.email=fixture@test.local"]
        def git(*args):
            return subprocess.run(command + list(args), cwd=self.fixture, env=self.env,
                                  input="", text=True, capture_output=True, check=True).stdout
        git("init", "-q")
        self.write(self.fixture / "sample.txt", "Needle\n")
        git("add", "sample.txt")
        git("commit", "-qm", "first")
        first = git("rev-parse", "HEAD").strip()
        self.write(self.fixture / "sample.txt", "Needle\nsecond\n")
        git("commit", "-qam", "second")
        self.assertEqual(git("gr", "needle"), "sample.txt:1:Needle\n")
        self.assertEqual(git("contributors"), git("shortlog", "-sn", "HEAD"))
        self.assertEqual(git("contributors", first + "..HEAD"),
                         git("shortlog", "-sn", first + "..HEAD"))
        self.assertIn("1\tFixture", git("contributors", first + "..HEAD"))

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
                        'bindkey "^[[A"; bindkey "^[[B"; bindkey "^[OA"; bindkey "^[OB"\n'
                        'for capability in khome kend kdch1 kcuu1 kcud1; do\n'
                        '  [[ -z ${terminfo[$capability]} ]] || bindkey "${terminfo[$capability]}"\n'
                        'done\n', interactive=True,
                    )
                    lines = result.stdout.splitlines()
                    self.assertEqual(lines[0], "bindkey -A emacs main")
                    self.assertEqual([line.split()[-1] for line in lines[1:3]],
                                     ["beginning-of-line", "end-of-line"])
                    self.assertEqual([line.split()[-1] for line in lines[3:7]],
                                     ["up-line-or-beginning-search", "down-line-or-beginning-search",
                                      "up-line-or-beginning-search", "down-line-or-beginning-search"])
                    self.assertTrue(all(line.split()[-1] in
                                        ("beginning-of-line", "end-of-line", "delete-char",
                                         "up-line-or-beginning-search", "down-line-or-beginning-search")
                                        for line in lines[7:]))
        result = self.run_shell(
            'zmodload() { return 1; }\n'
            'source "$ROOT/symlinks/config/zsh/key-bindings.zsh"\n'
            'bindkey "\\e[H"; bindkey "\\e[F"; bindkey "\\e[3~"; '
            'bindkey "^[[A"; bindkey "^[[B"; bindkey "^[OA"; bindkey "^[OB"\n',
        )
        self.assertEqual([line.split()[-1] for line in result.stdout.splitlines()],
                         ["beginning-of-line", "end-of-line", "delete-char",
                          "up-line-or-beginning-search", "down-line-or-beginning-search",
                          "up-line-or-beginning-search", "down-line-or-beginning-search"])

    def test_readline_keeps_character_navigation_and_word_deletion_distinct(self):
        result = self.run_shell(
            'set -o emacs\n'
            'bind -f "$ROOT/symlinks/config/readline/inputrc"\n'
            'bind -p\n', shell="bash",
        )
        bindings = dict(line.rsplit(": ", 1) for line in result.stdout.splitlines()
                        if line.startswith('"') and ": " in line)
        self.assertEqual(bindings[r'"\C-b"'], "backward-char")
        self.assertEqual(bindings[r'"\e\C-?"'], "backward-kill-word")
        self.assertEqual(bindings[r'"\e[A"'], "history-search-backward")
        self.assertEqual(bindings[r'"\e[B"'], "history-search-forward")
        self.assertEqual(bindings[r'"\eOA"'], "history-search-backward")
        self.assertEqual(bindings[r'"\eOB"'], "history-search-forward")

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
