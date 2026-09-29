#!/usr/bin/env python3
"""Select CI work from repository inputs, without third-party dependencies."""

import json
import os
from pathlib import Path
import subprocess


SCRIPTS = ".github/workflows/scripts/"
LINUX = SCRIPTS + "linux/"
WINDOWS = SCRIPTS + "windows/"
FLAGS = (
    "run_docs_checks", "run_rust_checks", "run_rust_fmt", "run_audit", "run_deny",
    "run_mutation", "run_build_linux", "run_build_windows", "run_validate_config",
    "run_config_drift", "run_profile_linux", "run_profile_windows",
    "run_roundtrip_linux", "run_roundtrip_windows", "run_app_tests",
    "run_app_windows", "run_git_hooks", "run_hook_inputs", "run_wrapper_linux",
    "run_wrapper_windows", "run_lint", "run_managed_scripts", "run_prompt",
    "run_lock", "run_desktop_python", "run_stocks", "run_release",
)
APPS = {
    "git": {"application": "git", "packages": "git", "tests": "config aliases behavior clean"},
    "zsh": {"application": "zsh", "packages": "zsh fzf wget", "tests": "completion history configuration"},
    "vim": {"application": "vim", "packages": "vim", "tests": "opens configuration"},
    "nvim": {"application": "nvim", "packages": "neovim", "tests": "opens plugins configuration"},
}
LINTERS = {
    "shell": {
        "name": "ShellCheck",
        "setup": f"sh {LINUX}install-apt-packages.sh shellcheck",
        "test_function": "test_shellcheck",
    },
    "powershell": {
        "name": "PSScriptAnalyzer",
        "setup": 'pwsh -NoProfile -Command "Install-Module -Name PSScriptAnalyzer -Force -Scope CurrentUser"',
        "test_function": "test_psscriptanalyzer",
    },
}


def documentation(path: str) -> bool:
    # Source-tree assets can be embedded with include_str!, even Markdown.
    return not path.startswith("cli/src/") and (
        path.endswith(".md") or path.startswith(("docs/", ".agents/"))
    )


def binary_input(path: str) -> bool:
    if documentation(path):
        return False
    if path.startswith(("cli/tests/", "cli/benches/")) or path in {
        "cli/deny.toml", "cli/rustfmt.toml", "cli/.gitignore",
    }:
        return False
    return path.startswith(("cli/", ".cargo/")) or path in {
        "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml",
        ".github/workflows/release.yml", LINUX + "classify-ci-changes.py",
        LINUX + "classify-ci-changes.sh", LINUX + "classify-release.sh",
    }


def changed_paths(root: str, base: str, head: str) -> list[str]:
    # Include removed paths and both sides of renames, without Git's quoting or
    # newline ambiguity. Never turn an invalid comparison into a successful skip.
    result = subprocess.run(
        ["git", "-C", root, "diff", "--name-only", "--no-renames", "-z", base, head, "--"],
        check=True, stdout=subprocess.PIPE,
    )
    return [os.fsdecode(path) for path in result.stdout.split(b"\0") if path]


def classify(paths: list[str], *, full: bool = False) -> dict[str, object]:
    flags = dict.fromkeys(FLAGS, False)
    flags["run_docs_checks"] = True  # Links can point to any renamed/deleted file.
    apps: set[str] = set()
    linters: set[str] = set()

    def enable(*names: str) -> None:
        for name in names:
            flags["run_" + name] = True

    def profiles(*platforms: str) -> None:
        enable("validate_config", "config_drift")
        for platform in platforms:
            enable("profile_" + platform, "roundtrip_" + platform)

    def rust() -> None:
        enable("rust_checks", "rust_fmt", "build_linux", "build_windows")

    for path in paths:
        if documentation(path):
            continue
        if binary_input(path):
            enable("release")
        if path.endswith(".sh") or path in {"hooks/pre-commit"}:
            linters.add("shell")
        if path.endswith((".ps1", ".psm1")):
            linters.add("powershell")

        if path == ".github/workflows/ci.yml" or path in {
            LINUX + "classify-ci-changes.py", LINUX + "classify-ci-changes.sh",
            LINUX + "lib/test-helpers.sh", LINUX + "check.sh", WINDOWS + "Check.ps1",
        }:
            full = True
        elif path in {LINUX + "check-ci-contract.py", LINUX + "test-ci-changes.py"}:
            pass  # Always executed by classify-changes.
        elif path in {".github/workflows/release.yml", LINUX + "classify-release.sh"}:
            pass  # Publishing builds are selected independently of CI builds.
        elif path == LINUX + "test-docs.sh":
            pass
        elif path == LINUX + "test-static-analysis.sh":
            linters.update(LINTERS)
        elif path == LINUX + "test-hook-inputs.sh":
            enable("hook_inputs", "build_linux")
        elif path == LINUX + "test-stocks.sh":
            enable("stocks")
        elif path == LINUX + "test-git-hooks.sh" or path.startswith("hooks/"):
            enable("git_hooks", "hook_inputs")
        elif path in {"dotfiles.sh", LINUX + "test-shell-wrapper.sh"}:
            enable("wrapper_linux")
        elif path in {"dotfiles.ps1", WINDOWS + "Test-ShellWrapper.ps1"}:
            enable("wrapper_windows")
        elif path == LINUX + "test-uninstall.sh":
            enable("roundtrip_linux")
        elif path == WINDOWS + "Test-InstallUninstall.ps1":
            enable("roundtrip_windows")
        elif path == LINUX + "test-applications.sh":
            apps.update(APPS)
        elif path == WINDOWS + "Test-Applications.ps1":
            enable("app_windows")
        elif path == LINUX + "test-nvim-config.lua":
            apps.add("nvim")
        elif path == LINUX + "test-shell-config.py":
            apps.add("zsh")
        elif path == LINUX + "install-apt-packages.sh":
            apps.update(APPS)
            linters.add("shell")
        elif path == "cli/deny.toml":
            enable("deny")
        elif path in {"cli/rustfmt.toml", "rustfmt.toml", ".rustfmt.toml"}:
            enable("rust_fmt")
        elif path.startswith(("cli/tests/", "cli/benches/")):
            rust()
        elif binary_input(path):
            rust()
            profiles("linux", "windows")
            apps.update(APPS)
            enable("app_windows", "wrapper_linux", "wrapper_windows")
            if path.startswith("cli/src/") and path.endswith(".rs"):
                enable("mutation")
            if path.endswith(("Cargo.toml", "Cargo.lock")):
                enable("audit", "deny")
        elif path.startswith("conf/"):
            profiles("linux", "windows")
            if path in {"conf/symlinks.toml", "conf/git-config.toml", "conf/packages.toml"}:
                apps.update(APPS)
                enable("app_windows")
        elif path.startswith("system/"):
            profiles("linux")
        elif path.startswith("symlinks/"):
            # Config-drift tests inspect managed content as well as source paths.
            enable("validate_config", "config_drift")
            if path.startswith("symlinks/config/git/"):
                apps.add("git")
                enable("app_windows")
            if path.startswith(("symlinks/config/zsh/", "symlinks/config/bash/", "symlinks/config/shell/")) or path in {
                "symlinks/zshrc", "symlinks/zprofile", "symlinks/bashrc",
                "symlinks/bash_profile", "symlinks/config/wgetrc", "symlinks/config/git/windows",
            }:
                apps.add("zsh")
            if path.startswith("symlinks/vim/"):
                apps.update(("vim", "nvim"))
            if path.startswith("symlinks/config/powershell/"):
                enable("prompt")
            if path.startswith(("symlinks/config/hypr/scripts/", "symlinks/config/hypr/conf/")):
                enable("lock")
            if path.startswith("symlinks/config/quickshell/"):
                enable("desktop_python")
            if path == "symlinks/config/hypr/scripts/stocks.sh":
                enable("stocks")
        elif path.startswith(".github/workflows/") and not path.startswith(SCRIPTS):
            full = True
        elif path.startswith(".vscode/") or path in {
            ".github/CODEOWNERS", ".github/dependabot.yml", ".github/lsp.json",
            ".github/github-app.yml",
        }:
            # Known repository/editor metadata does not ship in the binary.
            pass
        elif path in {".gitignore", ".editorconfig", "LICENSE", "cli/.gitignore"}:
            pass
        else:
            # Unrecognized inputs may introduce a new build or test dependency.
            full = True
            enable("release")

    if full:
        # Full CI is not itself a reason to publish an unchanged binary.
        release = flags["run_release"]
        mutation = flags["run_mutation"]
        flags = dict.fromkeys(FLAGS, True)
        flags["run_release"] = release
        flags["run_mutation"] = mutation
        apps.update(APPS)
        linters.update(LINTERS)

    flags["run_app_tests"] = bool(apps)
    flags["run_lint"] = bool(linters)
    flags["run_managed_scripts"] = any(flags["run_" + name] for name in (
        "prompt", "lock", "desktop_python",
    ))
    for platform in ("linux", "windows"):
        if any(flags["run_" + name + "_" + platform] for name in (
            "profile", "roundtrip", "wrapper",
        )):
            enable("build_" + platform)
    if flags["run_validate_config"] or flags["run_config_drift"] or apps:
        enable("build_linux")
    if flags["run_app_windows"]:
        enable("build_windows")

    return {
        **flags,
        "docs_only": bool(paths) and all(documentation(path) for path in paths) and not full,
        "lint_matrix": {"include": [value for name, value in LINTERS.items() if name in linters]},
        "app_matrix": {"include": [value for name, value in APPS.items() if name in apps]},
    }


def main() -> None:
    root = os.environ["DIR"]
    output = Path(os.environ["GITHUB_OUTPUT"])
    base, head = os.environ.get("BASE_SHA"), os.environ.get("HEAD_SHA")
    full = os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch" or not (base and head)
    paths = changed_paths(root, base, head) if base and head else []
    print("Full CI requested" if full else f"Classifying {len(paths)} changed paths")
    outputs = classify(paths, full=full)
    with output.open("a", encoding="utf-8") as stream:
        for name, value in outputs.items():
            stream.write(f"{name}={json.dumps(value, separators=(',', ':'))}\n")
    selected = [name for name in FLAGS if outputs[name]]
    print("Selected: " + ", ".join(selected))


if __name__ == "__main__":
    main()
