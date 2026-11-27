"""Regressions for shared shellcheck.sh and psscriptanalyzer.ps1 discovery and failures."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPTS_DIR = Path(__file__).resolve().parents[1]
SHELLCHECK_SCRIPT = SCRIPTS_DIR / "shellcheck.sh"
PSSCRIPTANALYZER_SCRIPT = SCRIPTS_DIR / "psscriptanalyzer.ps1"


def _write_inputs(root: Path, cases: tuple[tuple[str, str, bool], ...]) -> list[str]:
    nested = root / "hooks" / "nested"
    nested.mkdir(parents=True, exist_ok=True)
    expected: list[str] = []
    for name, contents, accepted in cases:
        (nested / name).write_text(contents, encoding="utf-8")
        if accepted:
            expected.append(name)
    return sorted(expected)


def _recorded(log: Path) -> list[str]:
    if not log.is_file():
        return []
    return sorted(log.read_text(encoding="utf-8").splitlines())


class SharedLinterScriptTests(unittest.TestCase):
    @unittest.skipIf(sys.platform == "win32", "POSIX sh discovery test runs on Unix")
    def test_shared_shell_discovery_and_failures(self) -> None:
        with tempfile.TemporaryDirectory(prefix="dotfiles-shellcheck-") as tmp_str:
            tmp = Path(tmp_str)
            root = tmp / "root o'connor [literal]"
            expected = _write_inputs(
                root,
                (
                    ("plain.sh", "echo hi", True),
                    ("space name.sh", "echo hi", True),
                    ("glob[name].sh", "echo hi", True),
                    ("sh", "#!/bin/sh -e\n", True),
                    ("bash", "#!/usr/local/bin/bash -x\n", True),
                    ("ksh", "#!/bin/ksh\n", True),
                    ("env", "#!/usr/bin/env -S dash -e\n", True),
                    ("exe", "#!/usr/bin/env.exe bash.exe\n", True),
                    ("tabs", "#!\t/usr/bin/env\tbash\n", True),
                    ("no newline", "#!/bin/bash", True),
                    ("skip.zsh", "#!/bin/bash\n", False),
                    ("zsh", "#!/usr/bin/env zsh\n", False),
                    ("fish", "#!/bin/fish\n", False),
                    ("csh", "#!/bin/csh\n", False),
                    ("tcsh", "#!/bin/tcsh\n", False),
                    ("python", "#!/bin/python3\n", False),
                    ("suffix", "#!/bin/notbash\n", False),
                    ("env missing", "#!/usr/bin/env -S\n", False),
                    ("empty", "", False),
                    ("not first line", "\n#!/bin/bash\n", False),
                ),
            )
            recipe = root / "packages" / "recipe with spaces"
            (recipe / "src").mkdir(parents=True)
            (recipe / "PKGBUILD").write_text("# shellcheck shell=bash\n", encoding="utf-8")
            (recipe / "version.sh").write_text("generated pins", encoding="utf-8")
            (recipe / "src" / "ignored.sh").write_text("generated source", encoding="utf-8")
            expected = sorted([*expected, "PKGBUILD"])

            mock_bin = tmp / "bin"
            mock_bin.mkdir()
            mock_sc = mock_bin / "shellcheck"
            mock_sc.write_text(
                "#!/bin/sh\n"
                "set -eu\n"
                "for file do\n"
                '  case "$file" in --*) continue ;; esac\n'
                '  basename "$file" >> "$LINT_INPUT_LOG"\n'
                "done\n"
                'exit "$LINT_EXIT"\n',
                encoding="utf-8",
            )
            mock_sc.chmod(0o755)
            log = tmp / "inputs"
            env = {
                **os.environ,
                "PATH": f"{mock_bin}:{os.environ.get('PATH', '')}",
                "LINT_INPUT_LOG": str(log),
            }

            def run_root(exit_code: str) -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    ["sh", str(SHELLCHECK_SCRIPT), "--root", str(root)],
                    env={**env, "LINT_EXIT": exit_code},
                    text=True,
                    capture_output=True,
                    check=False,
                )

            ok = run_root("0")
            self.assertEqual(ok.returncode, 0, ok.stderr)
            self.assertEqual(_recorded(log), expected)

            log.unlink()
            selected = root / "hooks" / "nested" / "space name.sh"
            sel = subprocess.run(
                ["sh", str(SHELLCHECK_SCRIPT), str(selected)],
                env={**env, "LINT_EXIT": "0"},
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(sel.returncode, 0, sel.stderr)
            self.assertEqual(_recorded(log), ["space name.sh"])

            self.assertNotEqual(run_root("1").returncode, 0, "linter findings must fail")

            (root / "hooks" / "broken").symlink_to("missing")
            self.assertNotEqual(run_root("0").returncode, 0, "broken inputs must fail discovery")

            shutil.rmtree(root / "hooks")
            (root / "hooks").write_text("not a directory", encoding="utf-8")
            self.assertNotEqual(run_root("0").returncode, 0, "non-directory inputs must fail")

            (root / "hooks").unlink()
            shutil.rmtree(root / "packages")
            self.assertEqual(
                run_root("1").returncode,
                0,
                "empty discovery must not invoke the tool",
            )

    def test_shared_powershell_discovery_and_failures(self) -> None:
        if shutil.which("pwsh") is None:
            self.skipTest("pwsh not installed")
        with tempfile.TemporaryDirectory(prefix="dotfiles-pssa-") as tmp_str:
            tmp = Path(tmp_str)
            root = tmp / "root o'connor [literal]"
            expected = _write_inputs(
                root,
                (
                    ("space name.ps1", "Write-Host hi", True),
                    ("glob[name].psm1", "function Test {}", True),
                    ("manifest.psd1", "@{}", True),
                    ("pwsh", "#!/usr/bin/env -S pwsh -NoProfile\n", True),
                    ("powershell", "#!/usr/bin/powershell.exe\n", True),
                    ("tabs", "#!\t/usr/bin/env.exe\tpwsh.exe\n", True),
                    ("no newline", "#!/usr/bin/pwsh", True),
                    ("shell", "#!/bin/sh\n", False),
                    ("suffix", "#!/bin/notpwsh\n", False),
                    ("empty", "", False),
                    ("not first line", "\n#!/usr/bin/pwsh\n", False),
                ),
            )
            modules = tmp / "modules"
            module = modules / "PSScriptAnalyzer"
            module.mkdir(parents=True)
            (module / "PSScriptAnalyzer.psm1").write_text(
                "function Invoke-ScriptAnalyzer {\n"
                "    param([string]$Path, [string[]]$Severity)\n"
                "    if (($Severity -join ',') -ne 'Warning,Error') { throw 'wrong severity' }\n"
                '    [IO.File]::AppendAllText($env:LINT_INPUT_LOG, [IO.Path]::GetFileName($Path) + "`n")\n'
                "    if ($env:LINT_EXIT -eq '1') { 'fixture finding' }\n"
                "    if ($env:LINT_EXIT -eq '2') { throw 'fixture analyzer failure' }\n"
                "}\n"
                "Export-ModuleMember -Function Invoke-ScriptAnalyzer\n",
                encoding="utf-8",
            )

            log_all = tmp / "inputs-all"
            log_selected = tmp / "inputs-selected"
            log_other = tmp / "inputs-other"
            selected = root / "hooks" / "nested" / "space name.ps1"
            hooks = root / "hooks"

            def ps(path: Path) -> str:
                return str(path).replace("'", "''")

            driver = (
                f"$env:PSModulePath = '{ps(modules)}' + [IO.Path]::PathSeparator + (Join-Path $PSHOME 'Modules')\n"
                "function Invoke-Case([string]$Name, [scriptblock]$Body) {\n"
                "    $global:LASTEXITCODE = 0\n"
                "    $failed = $false\n"
                "    try {\n"
                "        $null = & $Body\n"
                "        if ($LASTEXITCODE -ne 0) { $failed = $true }\n"
                "    } catch {\n"
                "        $failed = $true\n"
                "    }\n"
                "    \"$Name=$(if ($failed) { 'fail' } else { 'ok' })\"\n"
                "}\n"
                f"Invoke-Case 'ok' {{ $env:LINT_INPUT_LOG = '{ps(log_all)}'; $env:LINT_EXIT = '0'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' -Root '{ps(root)}' }}\n"
                f"Invoke-Case 'explicit' {{ $env:LINT_INPUT_LOG = '{ps(log_selected)}'; $env:LINT_EXIT = '0'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' '{ps(selected)}' }}\n"
                f"Invoke-Case 'finding' {{ $env:LINT_INPUT_LOG = '{ps(log_other)}'; $env:LINT_EXIT = '1'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' -Root '{ps(root)}' }}\n"
                f"Invoke-Case 'throw' {{ $env:LINT_INPUT_LOG = '{ps(log_other)}'; $env:LINT_EXIT = '2'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' -Root '{ps(root)}' }}\n"
                "Remove-Module PSScriptAnalyzer -ErrorAction SilentlyContinue\n"
                f"Remove-Item -LiteralPath '{ps(module)}' -Recurse -Force\n"
                f"Invoke-Case 'missing_module' {{ $env:LINT_EXIT = '0'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' -Root '{ps(root)}' }}\n"
                f"Remove-Item -LiteralPath '{ps(hooks)}' -Recurse -Force\n"
                f"Set-Content -LiteralPath '{ps(hooks)}' -Value 'not a directory'\n"
                f"Invoke-Case 'invalid_dir' {{ $env:LINT_EXIT = '0'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' -Root '{ps(root)}' }}\n"
                f"Remove-Item -LiteralPath '{ps(hooks)}' -Force\n"
                f"Invoke-Case 'empty' {{ $env:LINT_EXIT = '1'; & '{ps(PSSCRIPTANALYZER_SCRIPT)}' -Root '{ps(root)}' }}\n"
            )
            res = subprocess.run(
                ["pwsh", "-NoProfile", "-Command", driver],
                env={**os.environ, "PSModulePath": str(modules)},
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(res.returncode, 0, f"stdout:\n{res.stdout}\nstderr:\n{res.stderr}")
            self.assertEqual(
                res.stdout.splitlines(),
                [
                    "ok=ok",
                    "explicit=ok",
                    "finding=fail",
                    "throw=fail",
                    "missing_module=fail",
                    "invalid_dir=fail",
                    "empty=ok",
                ],
                res.stdout,
            )
            self.assertEqual(_recorded(log_all), expected)
            self.assertEqual(_recorded(log_selected), ["space name.ps1"])
