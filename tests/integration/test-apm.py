#!/usr/bin/env python3
"""Exercise user-scope deployment with the built CLI and real APM in a disposable home."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def test_deployment(binary: Path, fixture: Path) -> None:
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

    def run(*args, cwd=home):
        subprocess.run(args, cwd=cwd, env=env, check=True)

    run("git", "init", "-q", repo)
    common = ["--root", repo, "--profile", "base", "--non-interactive", "--fail-on-skip"]
    run(binary, "check", *common, "--only", "apm-plugins")
    update = [binary, "update", *common, "--no-repo-update", "--only"]
    run(*update, "symlinks,agent-packages")
    skills = home / ".agents/skills"
    for name in ("agent-customization", "audit", "ship"):
        if not (skills / name / "SKILL.md").is_file():
            raise AssertionError(f"missing deployed skill: {name}")

    # APM can report all refs current while a local package has changed.
    source = apm / "plugins/dot-workflow/.apm/skills/audit/SKILL.md"
    marker = "Local-only edit for the APM deployment test."
    with source.open("a", encoding="utf-8") as stream:
        stream.write(f"\n{marker}\n")
    run(*update, "agent-packages")
    if marker not in (skills / "audit/SKILL.md").read_text(encoding="utf-8"):
        raise AssertionError("agent-packages did not redeploy the local edit")
    run(apm_bin, "install", "-g", "--frozen")
    run(apm_bin, "audit", "--ci", cwd=home / ".apm")

    deployed = skills / "audit/SKILL.md"
    edited = deployed.read_bytes() + b"\nLocal destination edit.\n"
    deployed.write_bytes(edited)
    result = subprocess.run([apm_bin, "install", "-g", "--frozen"], cwd=home, env=env)
    if result.returncode != 1 or deployed.read_bytes() != edited:
        raise AssertionError("frozen install must reject drift and leave the local edit intact")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="dotfiles-apm-") as directory:
        test_deployment(Path(sys.argv[1]).resolve(strict=True), Path(directory))
