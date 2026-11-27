#!/usr/bin/env python3
"""Select CI work from repository inputs, without third-party dependencies."""

from collections.abc import Callable
from dataclasses import dataclass
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
LINTER_CONFIG_FILES = _META.LINTER_CONFIG_FILES
EXTENSIONLESS_PYTHON_SCRIPTS = _META.EXTENSIONLESS_PYTHON_SCRIPTS
python_script = _META.python_script
shell_script = _META.shell_script
powershell_script = _META.powershell_script
lua_script = _META.lua_script
structured_file = _META.structured_file


@dataclass(frozen=True)
class PathRule:
    """Declarative path-to-CI-effect rule."""

    exact: frozenset[str] = frozenset()
    prefixes: tuple[str, ...] = ()
    predicate: Callable[[str], bool] | None = None
    flags: tuple[str, ...] = ()
    apps: tuple[str, ...] = ()
    linters: tuple[str, ...] = ()
    profiles: tuple[str, ...] = ()
    rust: bool = False

    def matches(self, path: str) -> bool:
        if path in self.exact:
            return True
        if self.prefixes and path.startswith(self.prefixes):
            return True
        return self.predicate is not None and self.predicate(path)


SITE_EXACT_INPUTS = frozenset(
    {
        ".github/workflows/pages.yml",
        SCRIPTS + "check_docs.py",
        SCRIPTS + "check-doc-site.py",
        SCRIPTS + "docs-publication-base.py",
    }
)

CONTRACT_EXACT_INPUTS = frozenset(
    {
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "rust-toolchain",
        "rust-toolchain.toml",
        SCRIPTS + "check_docs.py",
    }
)

BINARY_EXCLUDED_EXACT = frozenset(
    {
        "cli/deny.toml",
        "cli/rustfmt.toml",
        "cli/.gitignore",
    }
)

BINARY_EXTRA_EXACT = frozenset(
    {
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "rust-toolchain",
        "rust-toolchain.toml",
        ".github/workflows/release.yml",
        SCRIPTS + "ci_metadata.py",
        SCRIPTS + "classify-ci-changes.py",
        LINUX + "classify-release.sh",
        LINUX + "release-tag.sh",
    }
)

APM_EXACT_INPUTS = frozenset(
    {
        "cli/src/domains/ai/apm.rs",
        "conf/symlinks.toml",
    }
)
APM_WINDOWS_PREFIX_INPUTS = ("symlinks/apm/", "cli/src/domains/ai/apm/")
APM_PREFIX_INPUTS = (*APM_WINDOWS_PREFIX_INPUTS, "packages/apm/")

FULL_CI_EXACT = frozenset(
    {
        ".github/workflows/ci.yml",
        SCRIPTS + "ci_metadata.py",
        SCRIPTS + "classify-ci-changes.py",
        LINUX + "ci-comparison-range.sh",
        SCRIPTS + "ci_fixture_home.py",
        "check.py",
        SCRIPTS + "pre-push.py",
    }
)

NOOP_EXACT = frozenset(
    {
        # Always executed by classify-changes.
        SCRIPTS + "check-ci-contract.py",
        SCRIPTS + "workflow_yaml.py",
        SCRIPTS + "test_ci_changes.py",
        SCRIPTS + "select-mutations.py",
        SCRIPTS + "run-mutations.py",
        SCRIPTS + "report-mutations.py",
        ".github/workflows/mutation.yml",
        LINUX + "install-actionlint.sh",
        # Publishing builds are selected independently of CI builds.
        ".github/workflows/release.yml",
        LINUX + "classify-release.sh",
        LINUX + "release-tag.sh",
        # Docs runners are covered via site_input / contract_input / always-on docs links.
        SCRIPTS + "check_docs.py",
        SCRIPTS + "check-doc-links.py",
        SCRIPTS + "check-doc-site.py",
        SCRIPTS + "docs-publication-base.py",
        # Known repository/editor metadata does not ship in the binary.
        ".github/CODEOWNERS",
        ".github/dependabot.yml",
        ".github/lsp.json",
        ".github/github-app.yml",
        ".gitignore",
        ".editorconfig",
        "LICENSE",
        "cli/.gitignore",
    }
)

NOOP_PREFIXES = (SCRIPTS + "ci_changes_tests/", ".vscode/")

ALL_LINTER_CONFIG_FILES = frozenset().union(*LINTER_CONFIG_FILES.values())

# First-match classification rules for targeted test/script/config inputs.
DISPATCH_RULES: tuple[PathRule, ...] = (
    PathRule(
        exact=frozenset(
            {
                "tests/desktop/quickshell/test-desktop-qml.sh",
                "tests/desktop/quickshell/test-desktop-session.sh",
                "tests/desktop/quickshell/test_desktop_session.py",
            }
        ),
        prefixes=("tests/desktop/quickshell/qml/",),
        flags=("desktop_qml",),
    ),
    PathRule(prefixes=("tests/desktop/quickshell/python/",), flags=("desktop_python",)),
    PathRule(prefixes=("tests/desktop/hypr/",), flags=("lock",)),
    PathRule(
        exact=frozenset(
            {
                "tests/vim/test_nvim.py",
                "tests/vim/test-nvim-config.lua",
                "tests/vim/test-nvim-lock.lua",
            }
        ),
        apps=("nvim",),
    ),
    PathRule(exact=frozenset({"tests/vim/test_vim.py"}), apps=("vim",)),
    PathRule(prefixes=("tests/vim/",), apps=("vim", "nvim")),
    PathRule(exact=frozenset({"tests/shell/Test-Prompt.ps1"}), flags=("prompt",)),
    PathRule(exact=frozenset({"tests/integration/test_apm.py"}), flags=("apm", "apm_windows")),
    PathRule(exact=frozenset({WINDOWS + "Install-Apm.ps1"}), flags=("apm_windows",)),
    PathRule(exact=frozenset({SCRIPTS + "check_lint.py"}), linters=tuple(LINTERS)),
    PathRule(
        exact=frozenset({"tests/hooks/test_hook_inputs.py"}),
        prefixes=("tests/hooks/hook_inputs_tests/",),
        flags=("hook_inputs", "build_linux"),
    ),
    PathRule(
        exact=frozenset({"tests/hooks/test_git_hooks.py", "tests/hooks/test_maintenance.py"}),
        prefixes=("tests/hooks/git_hook_tests/", "hooks/"),
        flags=("git_hooks", "hook_inputs"),
    ),
    PathRule(exact=_META.WRAPPER_PARITY_PATHS, flags=("wrapper_linux", "wrapper_windows")),
    PathRule(
        exact=_META.WRAPPER_LINUX_EXACT,
        prefixes=("tests/wrapper/linux/",),
        flags=("wrapper_linux",),
    ),
    # The Windows PowerShell 5.1 smoke uses the installed fixture CLI.
    PathRule(exact=frozenset({"dotfiles.ps1"}), flags=("wrapper_windows", "roundtrip_windows")),
    PathRule(
        exact=frozenset({"tests/wrapper/Test-ShellWrapper.ps1"}),
        prefixes=("tests/wrapper/windows/",),
        flags=("wrapper_windows",),
    ),
    PathRule(
        exact=frozenset({"tests/integration/test_install_uninstall.py"}),
        flags=("roundtrip_linux", "roundtrip_windows"),
    ),
    PathRule(
        exact=frozenset({"tests/integration/test-arch-acceptance.sh"}),
        flags=("roundtrip_linux",),
    ),
    PathRule(exact=frozenset({"tests/git/test_git.py"}), apps=("git",), flags=("app_windows",)),
    PathRule(
        exact=frozenset({"tests/shell/test_zsh.py", "tests/shell/test_shell_config.py"}),
        prefixes=("tests/shell/shell_config_tests/",),
        apps=("zsh",),
    ),
    PathRule(
        exact=frozenset({LINUX + "install-apt-packages.sh"}),
        apps=tuple(APPS),
        linters=("shell",),
    ),
    PathRule(exact=frozenset({"cli/deny.toml"}), flags=("deny",)),
    PathRule(
        exact=frozenset({"cli/rustfmt.toml", "rustfmt.toml", ".rustfmt.toml"}),
        flags=("rust_fmt",),
    ),
    PathRule(prefixes=("cli/tests/", "cli/benches/"), rust=True),
)

# Additive sub-rules for conf/ entries (applied in addition to both platform profiles).
CONF_RULES: tuple[PathRule, ...] = (
    PathRule(
        exact=frozenset({"conf/system-files.toml", "conf/chmod.toml"}), flags=("rust_checks",)
    ),
    # GraphicalUnitTests reads the declarations.
    PathRule(exact=frozenset({"conf/systemd-units.toml"}), flags=("lock",)),
    PathRule(
        exact=frozenset({"conf/symlinks.toml", "conf/git-config.toml", "conf/packages.toml"}),
        apps=tuple(APPS),
        flags=("app_windows",),
    ),
)

# Additive sub-rules for symlinks/ entries (all matching rules apply).
SYMLINK_RULES: tuple[PathRule, ...] = (
    PathRule(prefixes=("symlinks/config/git/templates/",), flags=("rust_checks",)),
    # Exercised by Rust config tests, test_shell_config.py (zsh),
    # and tests/desktop/hypr/test_launchers.py (lock).
    PathRule(
        exact=frozenset({"symlinks/local/bin/code-insiders"}),
        flags=("rust_checks", "lock"),
        apps=("zsh",),
    ),
    PathRule(
        exact=frozenset(
            {
                "symlinks/config/git/aliases",
                "symlinks/config/systemd/user/clean-home-tmp.service",
                "symlinks/config/systemd/user/clean-screenshots.service",
            }
        ),
        prefixes=("symlinks/config/git/templates/",),
        flags=("git_hooks",),
    ),
    PathRule(prefixes=("symlinks/config/git/",), apps=("git",), flags=("app_windows",)),
    PathRule(
        exact=frozenset(
            {
                "symlinks/zshrc",
                "symlinks/zprofile",
                "symlinks/bashrc",
                "symlinks/bash_profile",
                "symlinks/config/wgetrc",
                "symlinks/config/git/windows",
                "symlinks/config/git/windows-desktop",
                "symlinks/config/git/ignore",
                "symlinks/config/git/aliases",
                "symlinks/config/user-dirs.dirs",
                "symlinks/config/systemd/user/xdg-user-directories.service",
                "symlinks/config/uwsm/env-hyprland.d/session.sh",
                "symlinks/config/tmux/tmux.conf",
                "symlinks/config/readline/inputrc",
            }
        ),
        prefixes=("symlinks/config/zsh/", "symlinks/config/bash/", "symlinks/config/shell/"),
        apps=("zsh",),
    ),
    PathRule(prefixes=("symlinks/vim/",), apps=("vim", "nvim")),
    PathRule(prefixes=("symlinks/config/powershell/",), flags=("prompt",)),
    PathRule(
        exact=frozenset({"symlinks/config/powershell/Microsoft.PowerShell_profile.ps1"}),
        predicate=lambda p: (
            p.startswith("symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_")
            and p.endswith("/LocalState/settings.json")
        ),
        flags=("roundtrip_windows",),
    ),
    PathRule(
        exact=frozenset({"symlinks/config/systemd/user/dotfiles-first-login.service"}),
        flags=("rust_checks", "roundtrip_linux"),
    ),
    PathRule(
        exact=frozenset({"symlinks/config/hypr/hypridle.conf"}),
        prefixes=("symlinks/config/hypr/scripts/", "symlinks/config/hypr/conf/"),
        flags=("lock",),
    ),
    PathRule(
        exact=frozenset(
            {"symlinks/config/hypr/hyprland.lua", "symlinks/config/hypr/hyprlock.conf"}
        ),
        prefixes=("symlinks/config/hypr/conf/",),
        flags=("desktop_qml",),
    ),
    PathRule(
        predicate=lambda p: (
            p.rpartition("/")[0] == "symlinks/config/systemd/user"
            and p.endswith((".service", ".timer"))
        ),
        flags=("desktop_qml",),
    ),
    # The Hypr script suite follows graphical declarations to shipped
    # service files and exercises the first-login queue commands.
    PathRule(
        predicate=lambda p: (
            p.rpartition("/")[0] == "symlinks/config/systemd/user" and p.endswith(".service")
        ),
        flags=("lock",),
    ),
    PathRule(prefixes=("symlinks/config/quickshell/",), flags=("desktop_python", "desktop_qml")),
)


def documentation(path: str) -> bool:
    # Markdown can be compiled into the CLI or deployed as an APM skill.
    return not path.startswith(("cli/src/", "symlinks/apm/")) and (
        path.endswith(".md")
        or path.startswith(("docs/", ".agents/"))
        or path == ".github/workflows/pages.yml"
    )


def site_input(path: str) -> bool:
    return path.startswith("docs/") or path in SITE_EXACT_INPUTS


def contract_input(path: str) -> bool:
    return (
        (path.startswith("cli/") and not documentation(path))
        or path.startswith((".cargo/", "docs/content/"))
        or path in CONTRACT_EXACT_INPUTS
    )


def binary_input(path: str) -> bool:
    if documentation(path):
        return False
    if path.startswith(("cli/tests/", "cli/benches/")) or path in BINARY_EXCLUDED_EXACT:
        return False
    return path.startswith(("cli/", ".cargo/")) or path in BINARY_EXTRA_EXACT


def changed_paths(root: str, base: str, head: str) -> list[str]:
    # Include removed paths and both sides of renames, without Git's quoting or
    # newline ambiguity. Never turn an invalid comparison into a successful skip.
    result = subprocess.run(
        ["git", "-C", root, "diff", "--name-only", "--no-renames", "-z", base, head, "--"],
        check=True,
        stdout=subprocess.PIPE,
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

    def apply_rule(rule: PathRule) -> None:
        if rule.flags:
            enable(*rule.flags)
        if rule.apps:
            apps.update(rule.apps)
        if rule.linters:
            linters.update(rule.linters)
        if rule.profiles:
            profiles(*rule.profiles)
        if rule.rust:
            rust()

    for path in paths:
        if site_input(path):
            enable("docs_site")
        if contract_input(path):
            enable("docs_contracts")
        if documentation(path):
            continue
        if path.startswith(APM_PREFIX_INPUTS) or path in APM_EXACT_INPUTS:
            enable("apm")
        if path.startswith(APM_WINDOWS_PREFIX_INPUTS) or path in APM_EXACT_INPUTS:
            enable("apm_windows")
        if binary_input(path):
            enable("release")
        if shell_script(path):
            linters.add("shell")
        if powershell_script(path):
            linters.add("powershell")
        if python_script(path) or path in LINTER_CONFIG_FILES["python"]:
            linters.add("python")
        if lua_script(path) or path in LINTER_CONFIG_FILES["lua"]:
            linters.add("lua")
        if structured_file(path) or path in LINTER_CONFIG_FILES["structured"]:
            linters.add("structured")

        if path in FULL_CI_EXACT:
            full = True
            continue
        if path in NOOP_EXACT or path.startswith(NOOP_PREFIXES) or path in ALL_LINTER_CONFIG_FILES:
            continue

        matched = next((rule for rule in DISPATCH_RULES if rule.matches(path)), None)
        if matched is not None:
            apply_rule(matched)
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
            for rule in CONF_RULES:
                if rule.matches(path):
                    apply_rule(rule)
        elif path.startswith("system/"):
            profiles("linux")
            enable("rust_checks")
        elif path.startswith("symlinks/"):
            # Config-drift tests inspect managed content as well as source paths.
            enable("validate_config", "config_drift")
            for rule in SYMLINK_RULES:
                if rule.matches(path):
                    apply_rule(rule)
        elif path.startswith(".github/workflows/") and not path.startswith(SCRIPTS):
            full = True
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
        if any(
            flags["run_" + name + "_" + platform]
            for name in (
                "profile",
                "roundtrip",
                "wrapper",
            )
        ):
            enable("build_" + platform)
    if flags["run_apm"]:
        enable("validate_config")
    if flags["run_validate_config"] or flags["run_config_drift"] or apps:
        enable("build_linux")
    if flags["run_app_windows"] or flags["run_apm_windows"]:
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
        outputs["run_docs_site"] = (
            outputs["run_docs_site"]
            or not published
            or any(site_input(path) for path in changed_paths(root, published, head or "HEAD"))
        )
    with output.open("a", encoding="utf-8") as stream:
        for name, value in outputs.items():
            stream.write(f"{name}={json.dumps(value, separators=(',', ':'))}\n")
    selected = [name for name in FLAGS if outputs[name]]
    print("Selected: " + ", ".join(selected))


if __name__ == "__main__":
    main()
