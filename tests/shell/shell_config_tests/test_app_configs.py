"""Isolated tests for OpenSSH, makepkg, package install hooks, and desktop entries."""

from __future__ import annotations

import configparser
import shutil
import subprocess

from shell_config_tests.base import ROOT, ShellConfigTestCase


def _parse_desktop_exec(raw_value: str) -> list[str]:
    """Decode Desktop Entry Specification string escapes and Exec quoting."""
    unescaped: list[str] = []
    index = 0
    length = len(raw_value)
    string_escapes = {"s": " ", "n": "\n", "t": "\t", "r": "\r", "\\": "\\"}
    while index < length:
        ch = raw_value[index]
        if ch == "\\" and index + 1 < length and raw_value[index + 1] in string_escapes:
            unescaped.append(string_escapes[raw_value[index + 1]])
            index += 2
        else:
            unescaped.append(ch)
            index += 1
    text = "".join(unescaped)

    argv: list[str] = []
    current: list[str] = []
    in_quotes = False
    index = 0
    length = len(text)
    while index < length:
        ch = text[index]
        if in_quotes:
            if ch == "\\" and index + 1 < length and text[index + 1] in {'"', "`", "$", "\\"}:
                current.append(text[index + 1])
                index += 2
            elif ch == '"':
                in_quotes = False
                index += 1
            else:
                current.append(ch)
                index += 1
        elif ch == '"':
            in_quotes = True
            index += 1
        elif ch.isspace():
            if current:
                argv.append("".join(current))
                current = []
            index += 1
        else:
            current.append(ch)
            index += 1
    if in_quotes:
        raise ValueError(f"unterminated quote in Exec value: {raw_value!r}")
    if current:
        argv.append("".join(current))
    return argv


def _ssh_resolve(config_path: str, host: str, env: dict[str, str]) -> dict[str, list[str]]:
    result = subprocess.run(
        ["ssh", "-F", config_path, "-G", host],
        env=env,
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    if result.returncode != 0:
        raise AssertionError(f"ssh -G {host} failed:\n{result.stdout}{result.stderr}")
    options: dict[str, list[str]] = {}
    for line in result.stdout.splitlines():
        key, _, value = line.partition(" ")
        if key:
            options.setdefault(key.lower(), []).append(value.strip())
    return options


class AppConfigTests(ShellConfigTestCase):
    def test_ssh_config_host_resolution(self) -> None:
        if shutil.which("ssh") is None:
            self.skipTest("ssh not installed")
        config = str(ROOT / "symlinks/ssh/config")

        github = _ssh_resolve(config, "github.com", self.env)
        self.assertEqual(github["user"], ["git"])
        self.assertEqual(github["hostname"], ["github.com"])
        self.assertEqual(github["loglevel"], ["ERROR"])
        self.assertEqual(github["identitiesonly"], ["yes"])
        self.assertEqual(github["identityfile"], ["~/.ssh/github/id_ed25519"])
        self.assertEqual(github["preferredauthentications"], ["publickey"])
        self.assertIn(github["tcpkeepalive"], (["no"], ["false"]))
        self.assertEqual(github["serveraliveinterval"], ["60"])
        self.assertEqual(github["serveralivecountmax"], ["2"])
        self.assertIn(github["addkeystoagent"], (["yes"], ["true"]))

        internal = _ssh_resolve(config, "github-internal", self.env)
        self.assertEqual(internal["user"], ["git"])
        self.assertEqual(internal["hostname"], ["github.com"])
        self.assertEqual(internal["identityfile"], ["~/.ssh/github_internal/id_ed25519"])
        self.assertIn(internal["identitiesonly"], (["yes"], ["true"]))

        for ado_host in ("contoso.visualstudio.com", "ssh.dev.azure.com"):
            with self.subTest(host=ado_host):
                ado = _ssh_resolve(config, ado_host, self.env)
                self.assertEqual(ado["loglevel"], ["ERROR"])
                self.assertEqual(ado["identityfile"], ["~/.ssh/ado/id_ed25519"])
                self.assertIn(ado["identitiesonly"], (["yes"], ["true"]))
                self.assertEqual(ado["serveraliveinterval"], ["60"])

        generic = _ssh_resolve(config, "example.com", self.env)
        self.assertEqual(generic["serveraliveinterval"], ["60"])
        self.assertEqual(generic["serveralivecountmax"], ["2"])
        self.assertIn(generic["addkeystoagent"], (["yes"], ["true"]))
        self.assertNotIn("~/.ssh/github/id_ed25519", generic.get("identityfile", []))
        self.assertNotIn("~/.ssh/ado/id_ed25519", generic.get("identityfile", []))

    def test_makepkg_conf_rewrites_only_curl_dlagents(self) -> None:
        if shutil.which("bash") is None:
            self.skipTest("bash not installed")

        result = self.run_shell(
            """
            DLAGENTS=(
              'ftp::/usr/bin/curl -qgfC - --ftp-pasv --retry 3 --retry-delay 3 -o %o %u'
              'http::/usr/bin/curl -qgb "" -fLC - --retry 3 --retry-delay 3 -o %o %u'
              'https::/usr/bin/curl -qgb "" -fLC - --retry 3 --retry-delay 3 -o %o %u'
              'rsync::/usr/bin/rsync --no-motd -z %u %o'
              'scp::/usr/bin/scp -C %u %o'
            )
            . "$ROOT/symlinks/config/pacman/makepkg.conf"
            printf '%s\\n' "${DLAGENTS[@]}"
            printf 'LOOP_VAR=%s\\n' "${i-UNSET}"
            """,
            shell="bash",
        )
        lines = result.stdout.splitlines()
        self.assertEqual(
            lines,
            [
                "ftp::/usr/bin/curl -qgfC - --ftp-pasv --retry 3"
                " --retry-delay 3 --progress-bar -o %o %u",
                'http::/usr/bin/curl -qgb "" -fLC - --retry 3'
                " --retry-delay 3 --progress-bar -o %o %u",
                'https::/usr/bin/curl -qgb "" -fLC - --retry 3'
                " --retry-delay 3 --progress-bar -o %o %u",
                "rsync::/usr/bin/rsync --no-motd -z %u %o",
                "scp::/usr/bin/scp -C %u %o",
                "LOOP_VAR=UNSET",
            ],
        )

    def test_powershell_package_install_shells_lifecycle(self) -> None:
        install_script = ROOT / "packages/powershell/package.install"
        self.assertTrue(install_script.is_file(), f"missing {install_script}")

        # Case 1: etc/shells lacks a trailing newline; post_install adds one,
        # post_upgrade stays idempotent, pre_remove removes only the managed pair.
        root1 = self.fixture / "pkg-root-managed"
        shells1 = root1 / "etc/shells"
        shells1.parent.mkdir(parents=True)
        shells1.write_text("/bin/sh\n/bin/bash", encoding="utf-8")

        def run_install_hook(workdir, call: str) -> None:
            result = subprocess.run(
                ["sh", "-eu", "-c", f'. "{install_script}"\n{call}'],
                cwd=workdir,
                env=self.env,
                capture_output=True,
                text=True,
                check=False,
                timeout=15,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

        run_install_hook(root1, "post_install")
        self.assertEqual(
            shells1.read_text(encoding="utf-8"),
            "/bin/sh\n/bin/bash\n# Registered by powershell-dotfiles\n/usr/bin/pwsh\n",
        )

        run_install_hook(root1, "post_upgrade")
        self.assertEqual(
            shells1.read_text(encoding="utf-8"),
            "/bin/sh\n/bin/bash\n# Registered by powershell-dotfiles\n/usr/bin/pwsh\n",
        )

        run_install_hook(root1, "pre_remove")
        self.assertEqual(shells1.read_text(encoding="utf-8"), "/bin/sh\n/bin/bash\n")

        # Case 2: Pre-existing unmanaged /usr/bin/pwsh must be left untouched.
        root2 = self.fixture / "pkg-root-unmanaged"
        shells2 = root2 / "etc/shells"
        shells2.parent.mkdir(parents=True)
        shells2.write_text("/bin/sh\n/usr/bin/pwsh\n", encoding="utf-8")

        run_install_hook(root2, "post_install")
        self.assertEqual(shells2.read_text(encoding="utf-8"), "/bin/sh\n/usr/bin/pwsh\n")

        run_install_hook(root2, "pre_remove")
        self.assertEqual(shells2.read_text(encoding="utf-8"), "/bin/sh\n/usr/bin/pwsh\n")

    def test_desktop_entries_are_valid(self) -> None:
        if shutil.which("desktop-file-validate") is None:
            self.skipTest("desktop-file-validate not installed")

        desktop_files = sorted(
            [
                *(ROOT / "symlinks/local/share/applications").glob("*.desktop"),
                *(ROOT / "symlinks/config/autostart").glob("*.desktop"),
            ]
        )
        self.assertTrue(desktop_files, "expected tracked .desktop entries")
        result = subprocess.run(
            ["desktop-file-validate", *[str(path) for path in desktop_files]],
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
            timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_github_copilot_desktop_exec_delegates_to_launcher(self) -> None:
        entry = ROOT / "symlinks/local/share/applications/GitHub Copilot.desktop"
        parser = configparser.ConfigParser(interpolation=None)
        parser.read(entry, encoding="utf-8")
        argv = _parse_desktop_exec(parser.get("Desktop Entry", "Exec"))
        self.assertEqual(argv[:2], ["sh", "-c"])
        self.assertEqual(argv[-2:], ["sh", "%u"])
        shell_body = argv[2]
        self.assertIn('else exec /usr/bin/github "$@"', shell_body)

        # With the executable Hypr helper present, Exec delegates with %u.
        log = self.fixture / "launcher.log"
        self.write(
            self.home / ".config/hypr/scripts/launch-github.sh",
            f'#!/bin/sh\nprintf "LAUNCH:%s\\n" "$*" > "{log}"\n',
            executable=True,
        )
        delegated = subprocess.run(
            ["sh", "-c", shell_body, "sh", "github-app://open/repo"],
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
            timeout=15,
        )
        self.assertEqual(delegated.returncode, 0, delegated.stdout + delegated.stderr)
        self.assertEqual(log.read_text(encoding="utf-8").strip(), "LAUNCH:github-app://open/repo")

        # Without the helper, the fallback branch targets /usr/bin/github.
        (self.home / ".config/hypr/scripts/launch-github.sh").unlink()
        fallback_log = self.fixture / "fallback.log"
        fake_github = self.write(
            self.fixture / "bin/github",
            f'#!/bin/sh\nprintf "FALLBACK:%s\\n" "$*" > "{fallback_log}"\n',
            executable=True,
        )
        fallback_body = shell_body.replace("/usr/bin/github", str(fake_github))
        fallback = subprocess.run(
            ["sh", "-c", fallback_body, "sh", "github-app://fallback"],
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
            timeout=15,
        )
        self.assertEqual(fallback.returncode, 0, fallback.stdout + fallback.stderr)
        self.assertEqual(
            fallback_log.read_text(encoding="utf-8").strip(), "FALLBACK:github-app://fallback"
        )

    def test_wsl_conf_structure(self) -> None:
        wsl_conf = ROOT / "system/wsl.conf"
        parser = configparser.ConfigParser()
        parser.read(wsl_conf, encoding="utf-8")
        self.assertTrue(parser.getboolean("boot", "systemd"))
        self.assertFalse(parser.getboolean("interop", "appendWindowsPath"))
