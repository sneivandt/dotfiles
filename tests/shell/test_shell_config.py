#!/usr/bin/env python3
"""Isolated regressions for managed shell startup and command helpers.

Usage: python3 -B tests/shell/test_shell_config.py [--strict]
"""

from pathlib import Path
import sys

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
sys.path.insert(0, str(SCRIPT_DIR.parent))

from runner import pop_strict_flag, run_test_cases
from shell_config_tests.test_completion_and_tools import CompletionAndToolsTests
from shell_config_tests.test_extract_and_launchers import ExtractAndLaunchersTests
from shell_config_tests.test_profile_and_env import ProfileAndEnvTests
from shell_config_tests.test_ssh_prompt_and_system import SshPromptAndSystemTests

__all__ = [
    "CompletionAndToolsTests",
    "ExtractAndLaunchersTests",
    "ProfileAndEnvTests",
    "SshPromptAndSystemTests",
]


def main(argv: list[str] | None = None) -> int:
    strict, remaining = pop_strict_flag(list(sys.argv[1:] if argv is None else argv))
    if remaining:
        print(f"ERROR: unexpected arguments: {' '.join(remaining)}", file=sys.stderr)
        return 2
    return run_test_cases(
        CompletionAndToolsTests,
        ExtractAndLaunchersTests,
        ProfileAndEnvTests,
        SshPromptAndSystemTests,
        strict=strict,
    )


if __name__ == "__main__":
    raise SystemExit(main())
