#!/usr/bin/env python3
"""Verify shared behavioral constants and test-scenario parity across dotfiles.sh and dotfiles.ps1."""

from __future__ import annotations

import json
from pathlib import Path
import re
import unittest

WRAPPER_DIR = Path(__file__).resolve().parent
REPO_ROOT = WRAPPER_DIR.parents[1]
CONTRACT_PATH = WRAPPER_DIR / "parity-contract.json"


def _load_contract() -> dict:
    return json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))


class WrapperParityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.contract = _load_contract()
        cls.sh_text = (REPO_ROOT / "dotfiles.sh").read_text(encoding="utf-8")
        cls.ps1_text = (REPO_ROOT / "dotfiles.ps1").read_text(encoding="utf-8")
        cls.linux_runner = (WRAPPER_DIR / "test_shell_wrapper_linux.py").read_text(encoding="utf-8")
        cls.windows_runner = (WRAPPER_DIR / "Test-ShellWrapper.ps1").read_text(encoding="utf-8")

    def test_shared_timeouts_and_build_flags_match(self) -> None:
        constants = self.contract["constants"]
        connect = constants["connect_timeout_seconds"]
        transfer = constants["transfer_timeout_seconds"]
        self.assertRegex(self.sh_text, rf"(?m)^CONNECT_TIMEOUT={connect}\b")
        self.assertRegex(self.ps1_text, rf"(?m)^\$ConnectTimeout\s*=\s*{connect}\b")
        self.assertRegex(self.sh_text, rf"(?m)^TRANSFER_TIMEOUT={transfer}\b")
        self.assertRegex(self.ps1_text, rf"(?m)^\$TransferTimeout\s*=\s*{transfer}\b")

        build_cmd = (
            f"cargo build --profile {constants['cargo_profile']} "
            f"--bin {constants['cargo_bin']} "
            f"--message-format={constants['cargo_message_format']}"
        )
        self.assertIn(build_cmd, self.sh_text)
        self.assertIn(build_cmd, self.ps1_text)

    def test_shared_env_headers_assets_and_attestation_match(self) -> None:
        constants = self.contract["constants"]
        for name in constants["wrapper_env_vars"]:
            with self.subTest(env_var=name):
                self.assertIn(name, self.sh_text)
                self.assertIn(name, self.ps1_text)

        self.assertIn(f'DOTFILES_WRAPPER="{constants["wrapper_identifiers"]["sh"]}"', self.sh_text)
        self.assertIn(
            f'$env:DOTFILES_WRAPPER = "{constants["wrapper_identifiers"]["ps1"]}"', self.ps1_text
        )

        for token in (
            constants["github_api_accept"],
            constants["checksum_asset"],
            "attestation verify",
            "--repo",
            "disabled:",
            *constants["linux_assets"],
            *constants["attestation_env_vars"],
        ):
            with self.subTest(token=token):
                self.assertIn(token, self.sh_text)
                self.assertIn(token, self.ps1_text)

        for asset in constants["windows_assets"]:
            with self.subTest(windows_asset=asset):
                self.assertIn(asset, self.ps1_text)

        for prefix in constants["remote_prefixes"]:
            with self.subTest(remote_prefix=prefix):
                self.assertIn(prefix, self.sh_text)
                # PowerShell regex escapes dots in github\.com.
                escaped = prefix.replace(".", r"\.")
                self.assertTrue(
                    prefix in self.ps1_text or escaped in self.ps1_text,
                    f"dotfiles.ps1 missing remote prefix {prefix!r}",
                )

    def test_scenarios_are_defined_and_invoked_on_both_platforms(self) -> None:
        linux_defs: dict[str, set[str]] = {}
        for script in sorted((WRAPPER_DIR / "linux").glob("test_*.py")):
            text = script.read_text(encoding="utf-8")
            classes = re.findall(r"(?m)^class\s+([A-Za-z0-9_]+)\(unittest\.TestCase\):", text)
            methods = set(re.findall(r"(?m)^\s+def\s+(test_[A-Za-z0-9_]+)\(", text))
            for cls_name in classes:
                linux_defs[cls_name] = methods
        all_linux_defs = set().union(*linux_defs.values()) if linux_defs else set()
        for cls_name in linux_defs:
            self.assertIn(
                cls_name,
                self.linux_runner,
                f"Linux wrapper test class {cls_name!r} is not registered in tests/wrapper/test_shell_wrapper_linux.py",
            )

        windows_defs: set[str] = set()
        for script in sorted((WRAPPER_DIR / "windows").glob("*.ps1")):
            windows_defs.update(
                re.findall(
                    r"(?m)^function\s+([A-Za-z0-9_-]+)\b", script.read_text(encoding="utf-8")
                )
            )

        scenarios = self.contract["scenarios"]
        self.assertTrue(scenarios, "parity-contract.json must declare wrapper parity scenarios")
        for scenario, mapping in scenarios.items():
            linux_tests = mapping.get("linux") or []
            windows_tests = mapping.get("windows") or []
            self.assertTrue(linux_tests, f"Scenario {scenario!r} has no Linux wrapper test")
            self.assertTrue(windows_tests, f"Scenario {scenario!r} has no Windows wrapper test")

            for fn in linux_tests:
                with self.subTest(scenario=scenario, platform="linux", test=fn):
                    self.assertIn(
                        fn,
                        all_linux_defs,
                        f"Linux wrapper test {fn!r} is not defined under tests/wrapper/linux/",
                    )

            for fn in windows_tests:
                with self.subTest(scenario=scenario, platform="windows", test=fn):
                    self.assertIn(
                        fn,
                        windows_defs,
                        f"Windows wrapper test {fn!r} is not defined under tests/wrapper/windows/",
                    )
                    self.assertRegex(
                        self.windows_runner,
                        rf"(?m)^\s*\$results\s*\+=\s*{re.escape(fn)}\b",
                        f"Windows wrapper test {fn!r} is not invoked in tests/wrapper/Test-ShellWrapper.ps1",
                    )


if __name__ == "__main__":
    unittest.main()
