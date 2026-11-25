#!/usr/bin/env python3
"""Isolated regressions for managed shell startup and command helpers."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

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

if __name__ == "__main__":
    unittest.main()
