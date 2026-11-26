"""Shared CI workflow metadata consumed by classification, contract, and pre-push scripts."""

from pathlib import Path
import re
import subprocess
import sys

SCRIPTS = ".github/workflows/scripts/"
LINUX = SCRIPTS + "linux/"
WINDOWS = SCRIPTS + "windows/"

INFORMATIONAL_JOBS = frozenset({"coverage"})

JOB_CONDITIONS = {
    "rust-fmt": "run_rust_fmt",
    "lint": "run_lint",
    "powershell-prompt": "run_prompt",
    "desktop-script-regressions": "run_desktop_scripts",
    "docs": "run_docs_site",
    "docs-links": "run_docs_checks",
    "docs-contracts": "run_docs_contracts",
    "desktop-qml": "run_desktop_qml",
    "validate-config": "run_validate_config",
    "audit": "run_audit",
    "deny": "run_deny",
    "build-linux": "run_build_linux",
    "msrv": "run_rust_checks",
    "coverage": "run_rust_checks",
    "build-windows": "run_build_windows",
    "integration-linux": "run_profile_linux",
    "integration-windows": "run_profile_windows",
    "test-install-uninstall": "run_roundtrip_linux",
    "test-install-uninstall-windows": "run_roundtrip_windows",
    "test-applications": "run_app_tests",
    "test-applications-windows": "run_app_windows",
    "test-git-hooks": "run_git_hooks",
    "test-shell-wrapper-linux": "run_wrapper_linux",
    "test-shell-wrapper-windows": "run_wrapper_windows",
    "test-hook-inputs": "run_hook_inputs",
}

SPECIAL_CONDITIONS = {
    "validate-config": "needs.classify-changes.outputs.run_validate_config == 'true' && needs.build-linux.result == 'success'",
}

# Classification flags not mapped 1:1 to a top-level job condition in ci.yml.
AUXILIARY_FLAGS = (
    "run_config_drift",
    "run_apm",
    "run_lock",
    "run_desktop_python",
    "run_release",
)

FLAGS = tuple(dict.fromkeys((*JOB_CONDITIONS.values(), *AUXILIARY_FLAGS)))

ARTIFACT_DEPENDENCIES = {
    "validate-config": "linux",
    "integration-linux": "linux",
    "integration-windows": "windows",
    "test-install-uninstall": "linux",
    "test-install-uninstall-windows": "windows",
    "test-applications": "linux",
    "test-applications-windows": "windows",
    "test-shell-wrapper-linux": "linux",
    "test-shell-wrapper-windows": "windows",
}

MATRIX_JOBS = {
    "lint": "lint_matrix",
    "test-applications": "app_matrix",
}

APPS = {
    "git": {
        "application": "git",
        "script": "tests/git/test-git.py",
        "packages": "git",
        "tests": "config aliases behavior clean",
    },
    "zsh": {
        "application": "zsh",
        "script": "tests/shell/test-zsh.py",
        "packages": "zsh fzf wget libarchive-tools bat zstd",
        "tests": "completion history configuration",
    },
    "vim": {
        "application": "vim",
        "script": "tests/vim/test-vim.py",
        "packages": "vim",
        "tests": "opens configuration",
    },
    "nvim": {
        "application": "nvim",
        "script": "tests/vim/test-nvim.py",
        "packages": "neovim",
        "tests": "opens plugins configuration",
    },
}

LINTERS = {
    "shell": {
        "name": "ShellCheck",
        "setup": f"sh {LINUX}install-apt-packages.sh shellcheck",
        "test_function": "test_shellcheck",
    },
    "powershell": {
        "name": "PSScriptAnalyzer",
        "setup": 'pwsh -NoProfile -Command "Install-Module -Name PSScriptAnalyzer -RequiredVersion 1.25.0 -Force -Scope CurrentUser"',
        "test_function": "test_psscriptanalyzer",
    },
    "python": {
        "name": "Python (Ruff)",
        "setup": f"sh {LINUX}install-ruff.sh",
        "test_function": "test_ruff",
    },
    "lua": {
        "name": "Lua (Luacheck)",
        "setup": f"sh {LINUX}install-luacheck.sh",
        "test_function": "test_luacheck",
    },
}

LINTER_STAGES = {spec["name"]: stage for stage, spec in LINTERS.items()}

EXTENSIONLESS_PYTHON_SCRIPTS = frozenset({
    "symlinks/local/bin/code-insiders",
})

SHELL_SCRIPT_EXACT = frozenset({
    "hooks/pre-commit",
    "symlinks/bashrc",
    "symlinks/bash_profile",
})

SHELL_SCRIPT_PREFIXES = (
    "symlinks/config/git/templates/hooks/",
    "symlinks/config/git/templates/hooks.local/",
)

SHELL_SCRIPT_EXTENSIONS = (".sh", ".bash")
POWERSHELL_EXTENSIONS = (".ps1", ".psm1", ".psd1")
LUA_EXTENSIONS = (".lua",)

LINTER_CONFIG_FILES = {
    "python": frozenset({".github/linters/ruff.toml", LINUX + "install-ruff.sh"}),
    "lua": frozenset({".github/linters/luacheckrc", LINUX + "install-luacheck.sh"}),
}

_PYTHON_SHEBANG_RE = re.compile(r"^python([0-9]+(\.[0-9]+)*)?$")
_SHEBANG_SCAN_DIRS = ("symlinks", "hooks", "tests", ".github")


def python_script(path: str) -> bool:
    return path.endswith(".py") or path in EXTENSIONLESS_PYTHON_SCRIPTS


def shell_script(path: str) -> bool:
    parts = path.split("/")
    return (
        path.endswith(SHELL_SCRIPT_EXTENSIONS)
        or path in SHELL_SCRIPT_EXACT
        or path.startswith(SHELL_SCRIPT_PREFIXES)
        or (
            len(parts) == 3
            and parts[0] == "packages"
            and (parts[2] == "PKGBUILD" or parts[2].endswith(".install"))
        )
    )


def powershell_script(path: str) -> bool:
    return path.endswith(POWERSHELL_EXTENSIONS)


def lua_script(path: str) -> bool:
    return path.endswith(LUA_EXTENSIONS)


def workflow_file(path: str) -> bool:
    parts = path.split("/")
    return (
        len(parts) == 3
        and parts[0] == ".github"
        and parts[1] == "workflows"
        and parts[2].endswith((".yml", ".yaml"))
    )


LINTER_MATCHERS = {
    "shell": shell_script,
    "powershell": powershell_script,
    "python": python_script,
    "lua": lua_script,
    "workflows": workflow_file,
}


def is_python_shebang(path: Path) -> bool:
    """Return whether a file starts with a Python interpreter shebang."""
    try:
        with path.open("rb") as stream:
            first_line = stream.readline(256)
    except OSError:
        return False
    if not first_line.startswith(b"#!"):
        return False
    words = first_line[2:].decode("latin-1").split()
    if not words:
        return False
    name = words[0].rsplit("/", 1)[-1]
    if name == "env":
        name = next((word for word in words[1:] if not word.startswith("-")), "")
    return bool(_PYTHON_SHEBANG_RE.match(name))


def ruff_inputs(root: Path) -> list[str]:
    """Return Ruff target paths: the repository root plus extensionless Python scripts."""
    inputs = [str(root)]
    for dirname in _SHEBANG_SCAN_DIRS:
        base = root / dirname
        if not base.is_dir():
            continue
        for candidate in sorted(base.rglob("*")):
            if candidate.is_symlink() or not candidate.is_file():
                continue
            if candidate.suffix in {".py", ".pyi"}:
                continue
            if is_python_shebang(candidate):
                inputs.append(str(candidate))
    return inputs


def snapshot_commit(
    root: Path,
    head: str,
    destination: Path,
    *,
    symlinks: bool | None = None,
    env: dict[str, str] | None = None,
) -> None:
    """Create a detached isolated checkout of a commit with hooks disabled."""
    subprocess.run(
        ["git", "clone", "--quiet", "--template=", "--shared", "--no-checkout", str(root), str(destination)],
        cwd=root,
        env=env,
        check=True,
    )
    hooks = destination.parent / "empty-hooks"
    hooks.mkdir(exist_ok=True)
    options = ["-c", f"core.hooksPath={hooks}"]
    use_symlinks = (sys.platform != "win32") if symlinks is None else symlinks
    if not use_symlinks:
        options.extend(["-c", "core.symlinks=false"])
    subprocess.run(
        ["git", "-C", str(destination), *options, "checkout", "--quiet", "--detach", head],
        cwd=destination,
        env=env,
        check=True,
    )


def _cli(argv: list[str]) -> int:
    if len(argv) == 2 and argv[0] in {"filter-linter", "has-linter"}:
        matcher = LINTER_MATCHERS.get(argv[1])
        if matcher is None:
            print(f"Unknown linter category: {argv[1]}", file=sys.stderr)
            return 2
        raw = sys.stdin.buffer.read()
        matched = [
            entry for entry in raw.split(b"\0")
            if entry and matcher(entry.decode("utf-8", errors="surrogateescape"))
        ]
        if argv[0] == "has-linter":
            return 0 if matched else 1
        if matched:
            sys.stdout.buffer.write(b"\0".join(matched) + b"\0")
        return 0
    if len(argv) == 2 and argv[0] == "ruff-inputs":
        targets = ruff_inputs(Path(argv[1]))
        sys.stdout.buffer.write(b"\0".join(item.encode("utf-8", errors="surrogateescape") for item in targets) + b"\0")
        return 0
    print("Usage: ci_metadata.py <filter-linter|has-linter <category>|ruff-inputs <dir>>", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(_cli(sys.argv[1:]))
