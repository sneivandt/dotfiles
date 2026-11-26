#!/usr/bin/env python3
"""Select CI work from repository inputs, without third-party dependencies."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys


def _load_sibling(name: str):
    if name in sys.modules:
        return sys.modules[name]
    path = Path(__file__).resolve().with_name(f"{name}.py")
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load sibling module {name} from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_META = _load_sibling("ci_metadata")
SCRIPTS = _META.SCRIPTS
LINUX = _META.LINUX
WINDOWS = _META.WINDOWS
FLAGS = _META.FLAGS
APPS = _META.APPS
LINTERS = _META.LINTERS
LINTER_STAGES = _META.LINTER_STAGES
EXTENSIONLESS_PYTHON_SCRIPTS = _META.EXTENSIONLESS_PYTHON_SCRIPTS


def documentation(path: str) -> bool:
    # Markdown can be compiled into the CLI or deployed as an APM skill.
    return not path.startswith(("cli/src/", "symlinks/apm/")) and (
        path.endswith(".md") or path.startswith(("docs/", ".agents/"))
        or path == ".github/workflows/pages.yml"
    )


def site_input(path: str) -> bool:
    return path.startswith("docs/") or path in {
        ".github/workflows/pages.yml", LINUX + "test-docs.sh",
        LINUX + "check-doc-site.py", LINUX + "docs-publication-base.py",
    }


def contract_input(path: str) -> bool:
    return (path.startswith("cli/") and not documentation(path)) or path.startswith((
        ".cargo/", "docs/content/",
    )) or path in {
        "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml",
        LINUX + "test-docs.sh",
    }


def binary_input(path: str) -> bool:
    if documentation(path):
        return False
    if path.startswith(("cli/tests/", "cli/benches/")) or path in {
        "cli/deny.toml", "cli/rustfmt.toml", "cli/.gitignore",
    }:
        return False
    return path.startswith(("cli/", ".cargo/")) or path in {
        "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml",
        ".github/workflows/release.yml", LINUX + "ci_metadata.py",
        LINUX + "classify-ci-changes.py", LINUX + "classify-ci-changes.sh",
        LINUX + "classify-release.sh", LINUX + "release-tag.sh",
    }


def python_script(path: str) -> bool:
    return path.endswith(".py") or path in EXTENSIONLESS_PYTHON_SCRIPTS


def shell_script(path: str) -> bool:
    parts = path.split("/")
    return (
        path.endswith((".sh", ".bash"))
        or path in {"hooks/pre-commit", "symlinks/bashrc", "symlinks/bash_profile"}
        or path.startswith((
            "symlinks/config/git/templates/hooks/",
            "symlinks/config/git/templates/hooks.local/",
        ))
        or (
            len(parts) == 3
            and parts[0] == "packages"
            and (parts[2] == "PKGBUILD" or parts[2].endswith(".install"))
        )
    )


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
        if site_input(path):
            enable("docs_site")
        if contract_input(path):
            enable("docs_contracts")
        if documentation(path):
            continue
        if path.startswith(("symlinks/apm/", "cli/src/domains/ai/apm/", "packages/apm/")) or path in {
            "cli/src/domains/ai/apm.rs", "conf/symlinks.toml",
        }:
            enable("apm")
        if binary_input(path):
            enable("release")
        if shell_script(path):
            linters.add("shell")
        if path.endswith((".ps1", ".psm1", ".psd1")):
            linters.add("powershell")
        if python_script(path) or path in {".github/linters/ruff.toml", LINUX + "install-ruff.sh"}:
            linters.add("python")
        if path.endswith(".lua") or path in {".github/linters/luacheckrc", LINUX + "install-luacheck.sh"}:
            linters.add("lua")

        if path == ".github/workflows/ci.yml" or path in {
            LINUX + "ci_metadata.py", LINUX + "classify-ci-changes.py",
            LINUX + "classify-ci-changes.sh", LINUX + "ci-comparison-range.sh",
            LINUX + "ci-fixture-home.sh", "tests/lib/test-helpers.sh",
            LINUX + "check.sh", WINDOWS + "Check.ps1", LINUX + "pre-push.py",
        }:
            full = True
        elif path in {
            LINUX + "check-ci-contract.py", LINUX + "workflow_yaml.py",
            LINUX + "test-ci-changes.py", LINUX + "select-mutations.py",
            LINUX + "run-mutations.py", LINUX + "report-mutations.py",
            ".github/workflows/mutation.yml",
        } or path.startswith(LINUX + "ci_changes_tests/"):
            pass  # Always executed by classify-changes.
        elif path in {".github/workflows/release.yml", LINUX + "classify-release.sh", LINUX + "release-tag.sh"}:
            pass  # Publishing builds are selected independently of CI builds.
        elif path in {".github/linters/ruff.toml", LINUX + "install-ruff.sh"}:
            linters.add("python")
        elif path in {".github/linters/luacheckrc", LINUX + "install-luacheck.sh"}:
            linters.add("lua")
        elif path in {"tests/desktop/quickshell/test-desktop-qml.sh",
                      "tests/desktop/quickshell/test-desktop-session.sh",
                      "tests/desktop/quickshell/test-desktop-session.py"} or path.startswith(
            "tests/desktop/quickshell/qml/"
        ):
            enable("desktop_qml")
        elif path.startswith("tests/desktop/quickshell/python/"):
            enable("desktop_python")
        elif path.startswith("tests/desktop/hypr/"):
            enable("lock")
        elif path in {"tests/vim/test-nvim.py", "tests/vim/test-nvim-config.lua", "tests/vim/test-nvim-lock.lua"}:
            apps.add("nvim")
        elif path == "tests/vim/test-vim.py":
            apps.add("vim")
        elif path.startswith("tests/vim/"):
            apps.update(("vim", "nvim"))
        elif path == "tests/shell/Test-Prompt.ps1":
            enable("prompt")
        elif path in {LINUX + "test-workflows.sh", LINUX + "install-actionlint.sh"}:
            pass  # Always executed by classify-changes.
        elif path == "tests/integration/test-apm.py":
            enable("apm")
        elif path in {LINUX + "test-docs.sh", LINUX + "check-doc-links.py", LINUX + "check-doc-site.py",
                      LINUX + "docs-publication-base.py"}:
            pass
        elif path == LINUX + "test-static-analysis.sh":
            linters.update(LINTERS)
        elif path == "tests/hooks/test-hook-inputs.py" or path.startswith("tests/hooks/hook_inputs_tests/"):
            enable("hook_inputs", "build_linux")
        elif path in {"tests/hooks/test-git-hooks.py", "tests/hooks/test-maintenance.py"} or path.startswith(
            ("tests/hooks/git_hook_tests/", "hooks/")
        ):
            enable("git_hooks", "hook_inputs")
        elif path in {"tests/wrapper/parity-contract.json", "tests/wrapper/test-parity.py"}:
            enable("wrapper_linux", "wrapper_windows")
        elif path in {"dotfiles.sh", "tests/wrapper/test-shell-wrapper.sh"} or path.startswith(
            "tests/wrapper/linux/"
        ):
            enable("wrapper_linux")
        elif path in {"dotfiles.ps1", "tests/wrapper/Test-ShellWrapper.ps1"} or path.startswith(
            "tests/wrapper/windows/"
        ):
            enable("wrapper_windows")
            if path == "dotfiles.ps1":
                # The Windows PowerShell 5.1 smoke uses the installed fixture CLI.
                enable("roundtrip_windows")
        elif path == "tests/integration/test-install-uninstall.py":
            enable("roundtrip_linux", "roundtrip_windows")
        elif path == "tests/integration/test-arch-acceptance.sh":
            enable("roundtrip_linux")
        elif path == "tests/git/test-git.py":
            apps.add("git")
            enable("app_windows")
        elif path in {"tests/shell/test-zsh.py", "tests/shell/test-shell-config.py"} or path.startswith(
            "tests/shell/shell_config_tests/"
        ):
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
            if path.endswith(("Cargo.toml", "Cargo.lock")):
                enable("audit", "deny")
        elif path.startswith("packages/"):
            # Recipes are runtime inputs, not binary release inputs. Linux tests
            # validate their real descriptors; ShellCheck covers PKGBUILDs.
            enable("rust_checks", "build_linux", "validate_config", "config_drift")
            linters.add("shell")
        elif path.startswith("conf/"):
            profiles("linux", "windows")
            if path in {"conf/system-files.toml", "conf/chmod.toml"}:
                enable("rust_checks")
            if path == "conf/systemd-units.toml":
                enable("lock")  # GraphicalUnitTests reads the declarations.
            if path in {"conf/symlinks.toml", "conf/git-config.toml", "conf/packages.toml"}:
                apps.update(APPS)
                enable("app_windows")
        elif path.startswith("system/"):
            profiles("linux")
            enable("rust_checks")
        elif path.startswith("symlinks/"):
            # Config-drift tests inspect managed content as well as source paths.
            enable("validate_config", "config_drift")
            if path.startswith("symlinks/config/git/templates/"):
                enable("rust_checks")
            if path == "symlinks/local/bin/code-insiders":
                # Exercised by Rust config tests, test-shell-config.py (zsh),
                # and tests/desktop/hypr/test_launchers.py (lock).
                enable("rust_checks", "lock")
                apps.add("zsh")
            if path.startswith("symlinks/config/git/templates/") or path in {
                "symlinks/config/git/aliases",
                "symlinks/config/systemd/user/clean-home-tmp.service",
                "symlinks/config/systemd/user/clean-screenshots.service",
            }:
                enable("git_hooks")
            if path.startswith("symlinks/config/git/"):
                apps.add("git")
                enable("app_windows")
            if path.startswith(("symlinks/config/zsh/", "symlinks/config/bash/", "symlinks/config/shell/")) or path in {
                "symlinks/zshrc", "symlinks/zprofile", "symlinks/bashrc",
                "symlinks/bash_profile", "symlinks/config/wgetrc", "symlinks/config/git/windows",
                "symlinks/config/git/windows-desktop", "symlinks/config/git/ignore",
                "symlinks/config/git/aliases", "symlinks/config/user-dirs.dirs",
                "symlinks/config/systemd/user/xdg-user-directories.service",
                "symlinks/config/uwsm/env-hyprland.d/session.sh",
                "symlinks/config/tmux/tmux.conf",
                "symlinks/config/readline/inputrc",
            }:
                apps.add("zsh")
            if path.startswith("symlinks/vim/"):
                apps.update(("vim", "nvim"))
            if path.startswith("symlinks/config/powershell/"):
                enable("prompt")
            if path == "symlinks/config/powershell/Microsoft.PowerShell_profile.ps1" or (
                path.startswith("symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_")
                and path.endswith("/LocalState/settings.json")
            ):
                enable("roundtrip_windows")
            if path == "symlinks/config/systemd/user/dotfiles-first-login.service":
                enable("rust_checks", "roundtrip_linux")
            if (
                path.startswith(("symlinks/config/hypr/scripts/", "symlinks/config/hypr/conf/"))
                or path == "symlinks/config/hypr/hypridle.conf"
            ):
                enable("lock")
            if path.startswith("symlinks/config/hypr/conf/") or path in {
                "symlinks/config/hypr/hyprland.lua", "symlinks/config/hypr/hyprlock.conf",
            }:
                enable("desktop_qml")
            if path.rpartition("/")[0] == "symlinks/config/systemd/user":
                if path.endswith((".service", ".timer")):
                    enable("desktop_qml")
                # The Hypr script suite follows graphical declarations to shipped
                # service files and exercises the first-login queue commands.
                if path.endswith(".service"):
                    enable("lock")
            if path.startswith("symlinks/config/quickshell/"):
                enable("desktop_python", "desktop_qml")
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
        flags = dict.fromkeys(FLAGS, True)
        flags["run_release"] = release
        apps.update(APPS)
        linters.update(LINTERS)

    flags["run_app_tests"] = bool(apps)
    flags["run_lint"] = bool(linters)
    flags["run_desktop_scripts"] = flags["run_lock"] or flags["run_desktop_python"]
    for platform in ("linux", "windows"):
        if any(flags["run_" + name + "_" + platform] for name in (
            "profile", "roundtrip", "wrapper",
        )):
            enable("build_" + platform)
    if flags["run_apm"]:
        enable("validate_config")
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
    full = (
        os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
        or os.environ.get("CI_FORCE_FULL") == "true"
        or not (base and head)
    )
    paths = changed_paths(root, base, head) if base and head else []
    print("Full CI requested" if full else f"Classifying {len(paths)} changed paths")
    outputs = classify(paths, full=full)
    if os.environ.get("GITHUB_EVENT_NAME") == "push":
        # CI success does not imply Pages success. Carry unpublished site inputs
        # forward even when the immediately preceding push already passed CI.
        published = os.environ.get("DOCS_BASE_SHA")
        outputs["run_docs_site"] = outputs["run_docs_site"] or not published or any(
            site_input(path) for path in changed_paths(root, published, head or "HEAD")
        )
    with output.open("a", encoding="utf-8") as stream:
        for name, value in outputs.items():
            stream.write(f"{name}={json.dumps(value, separators=(',', ':'))}\n")
    selected = [name for name in FLAGS if outputs[name]]
    print("Selected: " + ", ".join(selected))


if __name__ == "__main__":
    main()
