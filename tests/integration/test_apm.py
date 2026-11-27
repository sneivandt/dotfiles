#!/usr/bin/env python3
"""Exercise user-scope deployment with the built CLI and real APM in a disposable home.

Usage: python3 -B tests/integration/test_apm.py [--strict] [binary_path]
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tests"))

from runner import pop_strict_flag, run_test_cases


class ApmDeploymentTests(unittest.TestCase):
    def test_deployment(self) -> None:
        binary_env = os.environ.get("BINARY_PATH", "")
        self.assertTrue(binary_env, "BINARY_PATH must point to the built dotfiles CLI binary")
        binary = Path(binary_env).resolve(strict=True)
        with tempfile.TemporaryDirectory(prefix="dotfiles-apm-") as directory:
            self._run_deployment(binary, Path(directory))

    def _run_deployment(self, binary: Path, fixture: Path) -> None:
        home = fixture / "home"
        repo = fixture / "repo"
        apm = repo / "symlinks/apm"
        home.mkdir()
        conf = repo / "conf"
        conf.mkdir(parents=True)
        for manifest in (ROOT / "conf").glob("*.toml"):
            (conf / manifest.name).touch()
        (apm / "config").mkdir(parents=True)
        shutil.copyfile(ROOT / "symlinks/apm/config/base.yml", apm / "config/base.yml")
        shutil.copytree(ROOT / "symlinks/apm/plugins", apm / "plugins")
        # Select just the public local packages; no Arch dependencies or private overlays.
        (conf / "symlinks.toml").write_text(
            '[base]\nsymlinks = ["apm/config/base.yml", "apm/plugins/*"]\n',
            encoding="utf-8",
        )
        env = {
            "PATH": os.environ["PATH"],
            "HOME": str(home),
            "LANG": "C.UTF-8",
            "APM_HOME": str(home / ".apm"),
            "DOTFILES_SKIP_SELF_UPDATE": "1",
            "GIT_TERMINAL_PROMPT": "0",
        }
        if sys.platform == "win32":
            for key in (
                "SYSTEMROOT",
                "WINDIR",
                "COMSPEC",
                "PATHEXT",
                "OS",
                "PROCESSOR_ARCHITECTURE",
                "NUMBER_OF_PROCESSORS",
                "USERNAME",
                "TEMP",
                "TMP",
            ):
                if key in os.environ:
                    env[key] = os.environ[key]
            appdata = home / "AppData/Roaming"
            local_appdata = home / "AppData/Local"
            appdata.mkdir(parents=True, exist_ok=True)
            local_appdata.mkdir(parents=True, exist_ok=True)
            env["USERPROFILE"] = str(home)
            env["APPDATA"] = str(appdata)
            env["LOCALAPPDATA"] = str(local_appdata)
            env["PYTHONUTF8"] = "1"
            if home.drive:
                env["HOMEDRIVE"] = home.drive
                env["HOMEPATH"] = str(home)[len(home.drive) :]

        apm_bin = shutil.which("apm", path=env["PATH"]) or "apm"

        def run(*args: object, cwd: Path = home) -> None:
            subprocess.run([str(arg) for arg in args], cwd=cwd, env=env, check=True)

        run("git", "init", "-q", repo)
        common = ["--root", repo, "--profile", "base", "--non-interactive", "--fail-on-skip"]
        run(binary, "check", *common, "--only", "apm-plugins")
        update = [binary, "update", *common, "--no-repo-update", "--only"]
        run(*update, "symlinks,agent-packages")
        skills = home / ".agents/skills"
        for name in ("agent-customization", "audit", "ship"):
            self.assertTrue(
                (skills / name / "SKILL.md").is_file(), f"missing deployed skill: {name}"
            )

        # APM can report all refs current while a local package has changed.
        source = apm / "plugins/dot-workflow/.apm/skills/audit/SKILL.md"
        marker = "Local-only edit for the APM deployment test."
        with source.open("a", encoding="utf-8") as stream:
            stream.write(f"\n{marker}\n")
        run(*update, "agent-packages")
        self.assertIn(
            marker,
            (skills / "audit/SKILL.md").read_text(encoding="utf-8"),
            "agent-packages did not redeploy the local edit",
        )
        run(apm_bin, "install", "-g", "--frozen")
        run(apm_bin, "audit", "--ci", cwd=home / ".apm")

        deployed = skills / "audit/SKILL.md"
        edited = deployed.read_bytes() + b"\nLocal destination edit.\n"
        deployed.write_bytes(edited)
        result = subprocess.run([apm_bin, "install", "-g", "--frozen"], cwd=home, env=env)
        self.assertEqual(result.returncode, 1, "frozen install must reject drift")
        self.assertEqual(
            deployed.read_bytes(), edited, "frozen install must leave local edits intact"
        )


def main(argv: list[str]) -> int:
    strict, remaining = pop_strict_flag(argv)
    if len(remaining) > 1:
        print(f"ERROR: unexpected arguments: {' '.join(remaining)}", file=sys.stderr)
        return 2
    if remaining:
        os.environ["BINARY_PATH"] = remaining[0]
    return run_test_cases(ApmDeploymentTests, strict=strict)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
