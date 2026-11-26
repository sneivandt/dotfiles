"""Shared CI workflow metadata consumed by classification, contract, and pre-push scripts."""

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
