#!/usr/bin/env python3
"""Canonical cross-platform local verification entry point.

Runs the core repository checks used by CI in one command. Every stage is
optional-tool aware: if a stage's required tool is not installed it is reported
as SKIP rather than failing (unless --strict or DOTFILES_CHECK_STRICT=1 is set).

Usage:
  python3 -B .github/workflows/scripts/linux/check.py                 # run default stages
  python3 -B .github/workflows/scripts/linux/check.py fmt clippy      # run named stages
  python3 -B .github/workflows/scripts/linux/check.py --all           # include opt-in stages
  python3 -B .github/workflows/scripts/linux/check.py --list          # list available stages
"""

from collections.abc import Callable
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

CARGO_PROFILE = "ci"

LINUX_DEFAULT_STAGES = (
    "fmt", "clippy", "test", "rustdoc", "config", "docs", "ci",
    "shell", "powershell", "python", "lua", "audit", "deny",
)
LINUX_OPTIONAL_STAGES = ("msrv", "qml", "hooks", "wrapper", "desktop")

WINDOWS_DEFAULT_STAGES = (
    "fmt", "clippy", "test", "rustdoc", "config",
    "shell", "powershell", "audit", "deny",
)
WINDOWS_OPTIONAL_STAGES = ("msrv", "wrapper")


def platform_stages() -> tuple[tuple[str, ...], tuple[str, ...]]:
    if sys.platform == "win32":
        return WINDOWS_DEFAULT_STAGES, WINDOWS_OPTIONAL_STAGES
    return LINUX_DEFAULT_STAGES, LINUX_OPTIONAL_STAGES


class Console:
    def __init__(self) -> None:
        color = sys.stdout.isatty() and not os.environ.get("NO_COLOR")
        self.red = "\033[31m" if color else ""
        self.green = "\033[32m" if color else ""
        self.yellow = "\033[33m" if color else ""
        self.blue = "\033[34m" if color else ""
        self.dim = "\033[2m" if color else ""
        self.off = "\033[0m" if color else ""

    def heading(self, text: str) -> None:
        print(f"\n{self.blue}== {text} =={self.off}", flush=True)

    def note(self, text: str) -> None:
        print(f"{self.dim}{text}{self.off}", flush=True)


class Runner:
    def __init__(self, repo_root: Path, console: Console) -> None:
        self.repo_root = repo_root
        self.cli_root = repo_root / "cli"
        self.manifest = self.cli_root / "Cargo.toml"
        self.scripts_linux = repo_root / ".github/workflows/scripts/linux"
        self.console = console

    @staticmethod
    def have(tool: str) -> bool:
        return shutil.which(tool) is not None

    def run_cmd(
        self,
        argv: list[str],
        *,
        cwd: Path | None = None,
        env: dict[str, str] | None = None,
    ) -> str:
        try:
            result = subprocess.run(argv, cwd=cwd or self.cli_root, env=env, check=False)
        except OSError as error:
            self.console.note(f"Cannot run {argv[0]}: {error}")
            return "FAIL"
        return "pass" if result.returncode == 0 else "FAIL"

    def run_sequence(
        self,
        commands: list[tuple[list[str], Path | None]],
        *,
        env: dict[str, str] | None = None,
    ) -> str:
        for argv, cwd in commands:
            if self.run_cmd(argv, cwd=cwd, env=env) != "pass":
                return "FAIL"
        return "pass"

    def check_pinned_toolchain(self) -> bool:
        toolchain_file = self.cli_root / "rust-toolchain.toml"
        try:
            with toolchain_file.open("rb") as stream:
                expected = tomllib.load(stream).get("toolchain", {}).get("channel")
        except (OSError, tomllib.TOMLDecodeError):
            expected = None
        if not isinstance(expected, str) or not expected:
            print(f"Cannot read pinned Rust version from {toolchain_file}", file=sys.stderr, flush=True)
            return False
        if not self.have("rustc"):
            print("rustc not installed", file=sys.stderr, flush=True)
            return False
        try:
            result = subprocess.run(
                ["rustc", "--version"],
                cwd=self.cli_root,
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            print(f"Cannot run rustc: {error}", file=sys.stderr, flush=True)
            return False
        actual = result.stdout.strip()
        if result.returncode != 0 or not actual.startswith(f"rustc {expected} "):
            print(f"Pinned Rust {expected} required; PATH selected {actual}.", file=sys.stderr, flush=True)
            print(
                "Select the pinned toolchain as described in "
                "docs/content/development/contributing.md#selecting-the-pinned-toolchain.",
                file=sys.stderr,
                flush=True,
            )
            return False
        return True

    def cargo_stage(self, args: list[str], *, env: dict[str, str] | None = None) -> str:
        if not self.have("cargo"):
            self.console.note("cargo not installed; skipping")
            return "skip"
        if not self.check_pinned_toolchain():
            return "FAIL"
        return self.run_cmd(["cargo", *args], cwd=self.cli_root, env=env)

    def cargo_subcommand_available(self, name: str) -> bool:
        try:
            result = subprocess.run(
                ["cargo", name, "--version"],
                cwd=self.cli_root,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
        except OSError:
            return False
        return result.returncode == 0

    def stage_fmt(self) -> str:
        return self.cargo_stage(["fmt", "--check", "--manifest-path", str(self.manifest)])

    def stage_clippy(self) -> str:
        return self.cargo_stage([
            "clippy", "--locked", "--profile", CARGO_PROFILE,
            "--manifest-path", str(self.manifest),
            "--all-targets", "--", "-D", "warnings",
        ])

    def stage_test(self) -> str:
        return self.cargo_stage([
            "test", "--locked", "--profile", CARGO_PROFILE,
            "--manifest-path", str(self.manifest),
        ])

    def stage_rustdoc(self) -> str:
        env = os.environ | {"RUSTDOCFLAGS": "-D warnings"}
        return self.cargo_stage([
            "doc", "--locked", "--profile", CARGO_PROFILE,
            "--manifest-path", str(self.manifest),
            "--no-deps", "--document-private-items",
        ], env=env)

    def stage_config(self) -> str:
        return self.cargo_stage([
            "run", "--locked", "--profile", CARGO_PROFILE,
            "--manifest-path", str(self.manifest), "--",
            "check", "--root", str(self.repo_root), "--profile", "desktop",
            "--only", "config-diagnostics,symlink-sources,config-files",
        ])

    def stage_docs(self) -> str:
        if sys.platform == "win32":
            return self.run_cmd(
                [sys.executable, "-B", str(self.repo_root / "docs/check.py")],
                cwd=self.repo_root,
            )
        if self.have("cargo") and not self.check_pinned_toolchain():
            return "FAIL"
        env = os.environ | {"DIR": str(self.repo_root)}
        return self.run_cmd(
            ["sh", str(self.scripts_linux / "test-docs.sh"), "docs_links", "docs_task_selectors", "docs_site"],
            cwd=self.cli_root,
            env=env,
        )

    def stage_ci(self) -> str:
        status = self.run_sequence([
            ([sys.executable, "-B", str(self.scripts_linux / "check-ci-contract.py")], self.cli_root),
            ([sys.executable, "-B", str(self.scripts_linux / "test-ci-changes.py")], self.cli_root),
        ])
        if status != "pass":
            return "FAIL"
        if not self.have("actionlint"):
            self.console.note("actionlint not installed; workflow lint skipped")
            return "skip"
        return self.run_cmd(["sh", str(self.scripts_linux / "test-workflows.sh")], cwd=self.cli_root)

    def _static_analysis(self, tool: str, test_fn: str) -> str:
        if not self.have(tool):
            self.console.note(f"{tool} not installed; skipping")
            return "skip"
        if not self.have("sh"):
            self.console.note(f"no POSIX sh available; skipping {test_fn}")
            return "skip"
        env = os.environ | {"DIR": str(self.repo_root)}
        return self.run_cmd(
            ["sh", str(self.scripts_linux / "test-static-analysis.sh"), test_fn],
            cwd=self.cli_root,
            env=env,
        )

    def stage_shell(self) -> str:
        return self._static_analysis("shellcheck", "test_shellcheck")

    def stage_powershell(self) -> str:
        if not self.have("pwsh"):
            self.console.note("pwsh not installed; skipping")
            return "skip"
        if sys.platform == "win32":
            probe = subprocess.run(
                [
                    "pwsh", "-NoProfile", "-Command",
                    "if (Get-Module -ListAvailable -Name PSScriptAnalyzer) { exit 0 } else { exit 1 }",
                ],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if probe.returncode != 0:
                self.console.note(
                    "PSScriptAnalyzer not installed; skipping "
                    "(Install-Module PSScriptAnalyzer -Scope CurrentUser)"
                )
                return "skip"
            return self.run_cmd(
                [
                    "pwsh", "-NoProfile", "-File",
                    str(self.repo_root / "cli/src/app/validation/scripts/psscriptanalyzer.ps1"),
                    "-Root", str(self.repo_root),
                ],
                cwd=self.cli_root,
            )
        return self._static_analysis("pwsh", "test_psscriptanalyzer")

    def stage_python(self) -> str:
        return self._static_analysis("ruff", "test_ruff")

    def stage_lua(self) -> str:
        return self._static_analysis("luacheck", "test_luacheck")

    def stage_audit(self) -> str:
        if not self.have("cargo"):
            self.console.note("cargo not installed; skipping")
            return "skip"
        if not self.cargo_subcommand_available("audit"):
            self.console.note("cargo-audit not installed; skipping (cargo install cargo-audit)")
            return "skip"
        return self.run_cmd(
            ["cargo", "audit", "--file", str(self.cli_root / "Cargo.lock")],
            cwd=self.cli_root,
        )

    def stage_deny(self) -> str:
        if not self.have("cargo"):
            self.console.note("cargo not installed; skipping")
            return "skip"
        if not self.cargo_subcommand_available("deny"):
            self.console.note("cargo-deny not installed; skipping (cargo install cargo-deny)")
            return "skip"
        return self.run_cmd(
            ["cargo", "deny", "--manifest-path", str(self.manifest), "check", "all"],
            cwd=self.cli_root,
        )

    def stage_qml(self) -> str:
        qt_root = os.environ.get("QT_ROOT_DIR")
        candidates = [
            Path(qt_root) / "bin/qmltestrunner" if qt_root else None,
            Path("/usr/lib/qt6/bin/qmltestrunner"),
        ]
        has_runner = self.have("qmltestrunner") or any(
            candidate is not None and candidate.is_file() and os.access(candidate, os.X_OK)
            for candidate in candidates
        )
        if not has_runner:
            self.console.note("Qt Quick Test not installed; skipping (requires Qt >= 6.8)")
            return "skip"
        return self.run_cmd(
            ["sh", str(self.repo_root / "tests/desktop/quickshell/test-desktop-qml.sh")],
            cwd=self.cli_root,
        )

    def stage_msrv(self) -> str:
        if not self.have("rustup"):
            self.console.note("rustup not installed; skipping")
            return "skip"
        try:
            with self.manifest.open("rb") as stream:
                msrv = tomllib.load(stream).get("package", {}).get("rust-version")
        except (OSError, tomllib.TOMLDecodeError):
            msrv = None
        if not isinstance(msrv, str) or not msrv:
            self.console.note(f"no rust-version in {self.manifest}; skipping")
            return "skip"
        self.console.note(f"MSRV from Cargo.toml: {msrv}")
        try:
            subprocess.run(
                ["rustup", "toolchain", "install", msrv, "--profile", "minimal"],
                cwd=self.cli_root,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
        except OSError as error:
            self.console.note(f"Cannot run rustup: {error}")
            return "FAIL"
        return self.run_cmd([
            "cargo", f"+{msrv}", "check", "--locked", "--profile", CARGO_PROFILE,
            "--manifest-path", str(self.manifest), "--all-targets",
        ], cwd=self.cli_root)

    def stage_hooks(self) -> str:
        if not self.have("sh"):
            self.console.note("no POSIX sh available; skipping hook tests")
            return "skip"
        return self.run_sequence([
            ([sys.executable, "-B", str(self.repo_root / "tests/hooks/test-hook-inputs.py")], self.repo_root),
            ([sys.executable, "-B", str(self.repo_root / "tests/hooks/test-maintenance.py")], self.repo_root),
            ([sys.executable, "-B", str(self.repo_root / "tests/hooks/test-git-hooks.py")], self.repo_root),
        ])

    def stage_wrapper(self) -> str:
        commands: list[tuple[list[str], Path | None]] = [
            ([sys.executable, "-B", str(self.repo_root / "tests/wrapper/test-parity.py")], self.repo_root),
        ]
        if sys.platform == "win32":
            if not self.have("pwsh"):
                self.console.note("pwsh not installed; skipping")
                return "skip"
            commands.append((
                [
                    "pwsh", "-NoProfile", "-Command",
                    ". ./tests/wrapper/Test-ShellWrapper.ps1; "
                    "if (-not (Test-IsolatedWrapperPath)) { exit 1 }; "
                    "if (-not (Test-CargoArtifactPath)) { exit 1 }",
                ],
                self.repo_root,
            ))
        else:
            commands.append((
                [
                    "sh", str(self.repo_root / "tests/wrapper/test-shell-wrapper.sh"),
                    "test_wrapper_preserves_runtime_context",
                    "test_wrapper_uses_cargo_artifact",
                ],
                self.repo_root,
            ))
        return self.run_sequence(commands)

    def stage_desktop(self) -> str:
        return self.run_sequence([
            (
                [sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests/desktop/hypr", "-p", "test_*.py"],
                self.repo_root,
            ),
            (
                [sys.executable, "-B", "-m", "unittest", "discover", "-s", "tests/desktop/quickshell/python", "-p", "test_*.py"],
                self.repo_root,
            ),
        ])

    def stage_map(self) -> dict[str, Callable[[], str]]:
        return {
            "fmt": self.stage_fmt,
            "clippy": self.stage_clippy,
            "test": self.stage_test,
            "rustdoc": self.stage_rustdoc,
            "config": self.stage_config,
            "docs": self.stage_docs,
            "ci": self.stage_ci,
            "shell": self.stage_shell,
            "powershell": self.stage_powershell,
            "python": self.stage_python,
            "lua": self.stage_lua,
            "audit": self.stage_audit,
            "deny": self.stage_deny,
            "msrv": self.stage_msrv,
            "qml": self.stage_qml,
            "hooks": self.stage_hooks,
            "wrapper": self.stage_wrapper,
            "desktop": self.stage_desktop,
        }


def usage(default_stages: tuple[str, ...], optional_stages: tuple[str, ...]) -> str:
    return (
        "Usage: python3 -B .github/workflows/scripts/linux/check.py [--strict] [--all | --list | STAGE...]\n\n"
        f"Stages (default): {' '.join(default_stages)}\n"
        f"Stages (opt-in):  {' '.join(optional_stages)}\n\n"
        "Options:\n"
        "  --strict  Fail if any requested stage is skipped.\n"
        "  --all     Run every stage, including opt-in stages.\n"
        "  --list    List available stages and exit.\n"
        "  -h        Show this help and exit.\n"
    )


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    default_stages, optional_stages = platform_stages()
    known = default_stages + optional_stages
    console = Console()
    strict = os.environ.get("DOTFILES_CHECK_STRICT") == "1"

    filtered: list[str] = []
    for arg in args:
        if arg == "--strict":
            strict = True
        else:
            filtered.append(arg)

    if filtered and filtered[0] in {"-h", "--help"}:
        print(usage(default_stages, optional_stages), end="")
        return 0
    if filtered and filtered[0] == "--list":
        for stage in known:
            print(stage)
        return 0
    if filtered and filtered[0] == "--all":
        selected = list(known)
    elif filtered:
        selected = filtered
    else:
        selected = list(default_stages)

    for stage in selected:
        if stage not in known:
            print(f"{console.red}Unknown stage: {stage}{console.off}\n", file=sys.stderr)
            print(usage(default_stages, optional_stages), end="", file=sys.stderr)
            return 2

    repo_root = Path(__file__).resolve().parents[4]
    runner = Runner(repo_root, console)
    actions = runner.stage_map()

    results: list[tuple[str, str]] = []
    failed = False
    skipped = False

    for stage in selected:
        console.heading(stage)
        status = actions[stage]()
        results.append((stage, status))
        if status == "skip":
            skipped = True
            if strict:
                failed = True
        elif status != "pass":
            failed = True

    print(f"\n{console.blue}== summary =={console.off}")
    for name, status in results:
        if status == "pass":
            print(f"  {console.green}PASS{console.off} {name}")
        elif status == "skip":
            print(f"  {console.yellow}SKIP{console.off} {name}")
        else:
            print(f"  {console.red}FAIL{console.off} {name}")

    if failed:
        print(f"\n{console.red}Checks failed.{console.off}", file=sys.stderr)
        return 1
    if skipped:
        print(f"\n{console.yellow}Checks completed with skips; coverage is incomplete.{console.off}")
    else:
        print(f"\n{console.green}All requested checks passed.{console.off}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
