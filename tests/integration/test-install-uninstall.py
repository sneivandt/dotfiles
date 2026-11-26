#!/usr/bin/env python3
"""Cross-platform install/uninstall round-trip tests.

Expected environment:
  BINARY_PATH  path to pre-built dotfiles CLI binary
  DIR          repository root (defaults to this checkout)
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPO_ROOT = Path(os.environ.get("DIR") or Path(__file__).resolve().parents[2])


def home_dir() -> Path:
    if sys.platform == "win32":
        value = os.environ.get("USERPROFILE") or os.environ.get("HOME")
    else:
        value = os.environ.get("HOME")
    return Path(value) if value else Path.home()


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def link_state(path: Path) -> tuple[object, ...]:
    # Inspect the link itself rather than its target so a recreated link is
    # detected even when target content is unchanged.
    st = os.lstat(path)
    return (
        st.st_dev,
        st.st_ino,
        st.st_mtime_ns,
        st.st_ctime_ns,
        getattr(st, "st_birthtime_ns", None),
    )


def assert_symlink(path: Path, expected: Path) -> None:
    if not os.path.lexists(path):
        raise AssertionError(f"expected symlink, path does not exist: {path}")
    if not os.path.islink(path):
        raise AssertionError(f"expected symlink, got regular entry: {path}")
    try:
        actual_target = os.path.realpath(path, strict=True)
    except OSError as exc:
        raise AssertionError(f"symlink target missing or dangling: {path} ({exc})") from exc
    expected_target = os.path.realpath(expected, strict=True)
    if os.path.normcase(actual_target) != os.path.normcase(expected_target):
        raise AssertionError(
            f"symlink {path} points to '{actual_target}', expected '{expected_target}'"
        )
    print(f"PASS: symlink target: {path} -> {expected_target}")


def assert_materialized(path: Path, expected_source: Path, snapshot_hash: str) -> None:
    if not os.path.lexists(path):
        raise AssertionError(f"expected materialized file after uninstall, path missing: {path}")
    if os.path.islink(path):
        raise AssertionError(f"expected materialized file, still a symlink: {path}")
    if not path.is_file():
        raise AssertionError(f"expected regular file after uninstall: {path}")
    actual_hash = sha256(path)
    if actual_hash != sha256(expected_source) or actual_hash != snapshot_hash:
        raise AssertionError(f"materialized content does not match installed source: {path}")
    print(f"PASS: materialized content: {path}")


def assert_installed_unchanged(
    path: Path,
    expected_source: Path,
    snapshot_hash: str,
    previous_state: tuple[object, ...],
) -> None:
    assert_symlink(path, expected_source)
    if link_state(path) != previous_state or sha256(path) != snapshot_hash:
        raise AssertionError(f"second apply changed installed link or content: {path}")
    print(f"PASS: second apply preserved link and content: {path}")


def run_checked(command: list[str], *, env: dict[str, str] | None = None, description: str) -> None:
    result = subprocess.run(command, env=env, check=False)
    if result.returncode != 0:
        raise AssertionError(f"{description} failed with exit code {result.returncode}")


def expect_failure(fn, message: str) -> None:
    try:
        fn()
    except AssertionError:
        return
    raise AssertionError(message)


def test_assertion_helpers() -> None:
    print("=== Testing assertion helpers")
    with tempfile.TemporaryDirectory() as fixture_str:
        fixture = Path(fixture_str)
        source = fixture / "source"
        other = fixture / "other"
        link = fixture / "link"
        source.write_text("expected\n", encoding="utf-8")
        other.write_text("wrong\n", encoding="utf-8")
        os.symlink(source, link)

        assert_symlink(link, source)
        link.unlink()
        os.symlink(other, link)
        expect_failure(
            lambda: assert_symlink(link, source),
            "assert_symlink accepted the wrong target",
        )
        link.unlink()
        os.symlink(fixture / "missing", link)
        expect_failure(
            lambda: assert_symlink(link, source),
            "assert_symlink accepted a dangling target",
        )

        materialized = fixture / "materialized"
        shutil.copyfile(source, materialized)
        snapshot_hash = sha256(source)
        assert_materialized(materialized, source, snapshot_hash)
        materialized.write_text("changed\n", encoding="utf-8")
        expect_failure(
            lambda: assert_materialized(materialized, source, snapshot_hash),
            "assert_materialized accepted changed content",
        )

        link.unlink()
        os.symlink(source, link)
        state = link_state(link)
        assert_installed_unchanged(link, source, snapshot_hash, state)
        # Keep the old inode allocated to reliably distinguish a recreated link.
        link.rename(fixture / "previous-link")
        os.symlink(source, link)
        expect_failure(
            lambda: assert_installed_unchanged(link, source, snapshot_hash, state),
            "assert_installed_unchanged accepted a replaced link",
        )
        state = link_state(link)
        source.write_text("changed\n", encoding="utf-8")
        expect_failure(
            lambda: assert_installed_unchanged(link, source, snapshot_hash, state),
            "assert_installed_unchanged accepted changed content",
        )


def test_posix_round_trip(binary: Path) -> None:
    print("=== Testing install/uninstall round-trip (base profile)")
    home = home_dir()
    targets = [
        (home / ".bashrc", REPO_ROOT / "symlinks" / "bashrc"),
        (home / ".zshrc", REPO_ROOT / "symlinks" / "zshrc"),
        (home / ".config" / "git" / "config", REPO_ROOT / "symlinks" / "config" / "git" / "config"),
    ]
    install_cmd = [
        str(binary),
        "update",
        "--no-repo-update",
        "--root",
        str(REPO_ROOT),
        "-p",
        "base",
        "--skip",
        "agent-packages",
    ]
    run_checked(install_cmd, description="Install command")

    snapshots: list[tuple[Path, Path, str, tuple[object, ...]]] = []
    for dest, source in targets:
        assert_symlink(dest, source)
        snapshots.append((dest, source, sha256(dest), link_state(dest)))

    print("Running second install to check convergence...")
    run_checked(install_cmd, description="Second install command")
    for dest, source, digest, state in snapshots:
        assert_installed_unchanged(dest, source, digest, state)

    print("Running uninstall...")
    run_checked(
        [str(binary), "remove", "--root", str(REPO_ROOT), "-p", "base"],
        description="Uninstall command",
    )
    for dest, source, digest, _ in snapshots:
        assert_materialized(dest, source, digest)


def assert_windows_profile_startup(
    shell: str,
    profile_path: Path,
    expected_launcher: Path,
    version: str,
) -> None:
    env = os.environ.copy()
    env["DOTFILES_TEST_PROFILE"] = str(profile_path)
    env["DOTFILES_TEST_LAUNCHER"] = str(expected_launcher)
    command = (
        "$ErrorActionPreference = 'Stop'; "
        "Set-Variable -Name HOME -Value $env:USERPROFILE -Force; "
        ". $env:DOTFILES_TEST_PROFILE; "
        "if ((Get-Command dotfiles.cmd).Source -ne $env:DOTFILES_TEST_LAUNCHER) { "
        "throw 'The installed profile did not add the fixture launcher to PATH' }; "
        "dot --version; "
        "exit $LASTEXITCODE"
    )
    result = subprocess.run(
        [shell, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", command],
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    lines = result.stdout.splitlines()
    if result.returncode != 0 or lines != [version]:
        raise AssertionError(
            f"{shell} did not start the fixture CLI through the installed profile "
            f"(rc={result.returncode}, stdout={result.stdout!r}, stderr={result.stderr!r})"
        )
    print(f"PASS: {shell} started fixture CLI through installed profile")


def test_windows_round_trip(binary: Path) -> None:
    print("=== Testing install/uninstall round-trip (base profile + desktop Terminal)")
    home = home_dir()
    git_config = home / ".config" / "git" / "config"
    git_config_source = REPO_ROOT / "symlinks" / "config" / "git" / "config"
    ps_profile = home / "Documents" / "PowerShell" / "Microsoft.PowerShell_profile.ps1"
    ps_profile_source = (
        REPO_ROOT / "symlinks" / "config" / "powershell" / "Microsoft.PowerShell_profile.ps1"
    )
    legacy_profile = home / "Documents" / "WindowsPowerShell" / "Microsoft.PowerShell_profile.ps1"
    terminal_rel = (
        Path("AppData")
        / "Local"
        / "Packages"
        / "Microsoft.WindowsTerminal_8wekyb3d8bbwe"
        / "LocalState"
        / "settings.json"
    )
    terminal = home / terminal_rel
    terminal_source = REPO_ROOT / "symlinks" / terminal_rel
    launcher = home / ".local" / "bin" / "dotfiles.cmd"

    install_cmd = [
        str(binary),
        "update",
        "--no-repo-update",
        "--root",
        str(REPO_ROOT),
        "-p",
        "base",
        "--skip",
        "packages,agent-packages,vscode-extensions",
    ]
    run_checked(install_cmd, description="Install command")

    assert_symlink(git_config, git_config_source)
    assert_symlink(ps_profile, ps_profile_source)
    assert_symlink(legacy_profile, ps_profile_source)
    if os.path.lexists(terminal):
        raise AssertionError("Base install linked desktop-only Terminal settings")

    version_proc = subprocess.run(
        [str(binary), "--version"], text=True, capture_output=True, check=False
    )
    if version_proc.returncode != 0 or not version_proc.stdout.strip():
        raise AssertionError("Fixture CLI did not report its version")
    version = version_proc.stdout.strip()

    launcher_proc = subprocess.run(
        ["cmd.exe", "/d", "/c", str(launcher), "--version"],
        text=True,
        capture_output=True,
        check=False,
    )
    if launcher_proc.returncode != 0 or launcher_proc.stdout.splitlines() != [version]:
        raise AssertionError(
            f"The installed launcher did not start the fixture CLI "
            f"(rc={launcher_proc.returncode}, stdout={launcher_proc.stdout!r}, stderr={launcher_proc.stderr!r})"
        )

    wrapper_proc = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(REPO_ROOT / "dotfiles.ps1"),
            "--version",
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if wrapper_proc.returncode != 0 or wrapper_proc.stdout.splitlines() != [version]:
        raise AssertionError(
            f"The wrapper did not start the fixture CLI under Windows PowerShell "
            f"(rc={wrapper_proc.returncode}, stdout={wrapper_proc.stdout!r}, stderr={wrapper_proc.stderr!r})"
        )

    assert_windows_profile_startup("pwsh.exe", ps_profile, launcher, version)
    assert_windows_profile_startup("powershell.exe", legacy_profile, launcher, version)

    git_config_hash = sha256(git_config)
    ps_profile_hash = sha256(ps_profile)
    git_config_state = link_state(git_config)
    ps_profile_state = link_state(ps_profile)

    print("Running second install to check convergence...")
    run_checked(install_cmd, description="Second install command")
    assert_installed_unchanged(git_config, git_config_source, git_config_hash, git_config_state)
    assert_installed_unchanged(ps_profile, ps_profile_source, ps_profile_hash, ps_profile_state)

    run_checked(
        [
            str(binary),
            "update",
            "--no-repo-update",
            "--root",
            str(REPO_ROOT),
            "-p",
            "desktop",
            "--only",
            "symlinks",
        ],
        description="Desktop symlink install",
    )
    assert_symlink(terminal, terminal_source)
    for remote_dir in (".vscode-server", ".vscode-server-insiders"):
        if os.path.lexists(home / remote_dir):
            raise AssertionError(f"Windows desktop install linked Linux-only {remote_dir}")
    terminal_hash = sha256(terminal)

    print("Running uninstall...")
    run_checked(
        [str(binary), "remove", "--root", str(REPO_ROOT), "-p", "base"],
        description="Uninstall command",
    )
    assert_materialized(git_config, git_config_source, git_config_hash)
    assert_materialized(ps_profile, ps_profile_source, ps_profile_hash)
    assert_materialized(legacy_profile, ps_profile_source, ps_profile_hash)
    if os.path.lexists(launcher):
        raise AssertionError("Removal left the installed launcher")
    assert_symlink(terminal, terminal_source)

    run_checked(
        [
            str(binary),
            "remove",
            "--root",
            str(REPO_ROOT),
            "-p",
            "desktop",
            "--only",
            "symlinks",
        ],
        description="Desktop symlink removal",
    )
    assert_materialized(terminal, terminal_source, terminal_hash)


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        test_assertion_helpers()
        print("All assertion helper self-tests passed")
        return 0
    if argv:
        print(f"ERROR: unexpected arguments: {' '.join(argv)}", file=sys.stderr)
        return 2

    test_assertion_helpers()

    binary_env = os.environ.get("BINARY_PATH", "")
    if not binary_env:
        print("ERROR: BINARY_PATH is not set", file=sys.stderr)
        return 1
    binary = Path(binary_env)
    if not binary.is_file():
        print(f"ERROR: Binary not found: {binary}", file=sys.stderr)
        return 1

    if sys.platform == "win32":
        test_windows_round_trip(binary)
    else:
        test_posix_round_trip(binary)

    print("All install/uninstall tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
