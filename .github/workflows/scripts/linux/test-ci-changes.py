#!/usr/bin/env python3
"""Isolated regressions for CI selection, result gates, and release catch-up."""

import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent
POLICY = runpy.run_path(str(SCRIPTS / "classify-ci-changes.py"))
CONTRACT = runpy.run_path(str(SCRIPTS / "check-ci-contract.py"))
classify = POLICY["classify"]
LINUX = POLICY["LINUX"]
WINDOWS = POLICY["WINDOWS"]


class ClassificationTests(unittest.TestCase):
    def test_exact_job_selection(self):
        config = {"validate_config", "config_drift", "build_linux"}
        profiles = config | {
            "profile_linux", "profile_windows", "roundtrip_linux", "roundtrip_windows",
            "build_windows",
        }
        rust = {"rust_checks", "rust_fmt", "build_linux", "build_windows"}
        cases = [
            ("README.md", set()),
            ("docs/TESTING.md", set()),
            (".agents/skills/ci-cd-patterns/SKILL.md", set()),
            ("cli/README.md", set()),
            ("symlinks/vim/README.md", set()),
            (".github/dependabot.yml", set()),
            (".vscode/settings.json", set()),
            ("LICENSE", set()),
            ("cli/src/main.rs", rust | profiles | {
                "app_tests", "app_windows", "wrapper_linux", "wrapper_windows",
                "mutation", "release",
            }),
            ("cli/src/domains/ai/apm/scripts/workflow_autopilot.py", rust | profiles | {
                "app_tests", "app_windows", "wrapper_linux", "wrapper_windows", "release",
            }),
            ("cli/tests/config_drift.rs", rust),
            ("cli/tests/common/fixtures/data.toml", rust),
            ("cli/Cargo.lock", rust | profiles | {
                "app_tests", "app_windows", "wrapper_linux", "wrapper_windows",
                "audit", "deny", "release",
            }),
            ("cli/build.rs", rust | profiles | {
                "app_tests", "app_windows", "wrapper_linux", "wrapper_windows", "release",
            }),
            ("cli/rustfmt.toml", {"rust_fmt"}),
            ("cli/deny.toml", {"deny"}),
            ("conf/packages.toml", profiles | {"app_tests", "app_windows"}),
            ("conf/agent-settings.toml", profiles),
            ("system/pam.d/login", config | {"profile_linux", "roundtrip_linux"}),
            ("symlinks/config/git/config", config | {"app_tests", "app_windows", "build_windows"}),
            ("symlinks/config/git/windows", config | {"app_tests", "app_windows", "build_windows"}),
            ("symlinks/vim/init.vim", config | {"app_tests"}),
            ("symlinks/config/zsh/prompt.zsh", config | {"app_tests"}),
            ("symlinks/bash_profile", config | {"app_tests"}),
            ("symlinks/bashrc", config | {"app_tests"}),
            ("symlinks/config/wgetrc", config | {"app_tests"}),
            ("symlinks/config/hypr/conf/appearance.lua", config | {"managed_scripts", "lock"}),
            ("symlinks/config/hypr/conf/resize-on-border.lua",
             config | {"managed_scripts", "lock"}),
            ("symlinks/config/powershell/tests/Test-Prompt.ps1",
             config | {"lint", "managed_scripts", "prompt"}),
            ("symlinks/config/hypr/scripts/lock-screen.sh",
             config | {"lint", "managed_scripts", "lock"}),
            ("symlinks/config/hypr/scripts/tests/test_lock_screen.py",
             config | {"managed_scripts", "lock"}),
            ("symlinks/config/quickshell/network_helper.py",
             config | {"managed_scripts", "desktop_python"}),
            ("symlinks/config/hypr/scripts/stocks.sh",
             config | {"stocks", "lint", "managed_scripts", "lock"}),
            ("symlinks/config/hypr/scripts/screenshot.sh",
             config | {"lint", "managed_scripts", "lock"}),
            ("symlinks/config/hypr/scripts/choose-editor.sh",
             config | {"lint", "managed_scripts", "lock"}),
            ("dotfiles.sh", {"lint", "wrapper_linux", "build_linux"}),
            ("dotfiles.ps1", {"lint", "wrapper_windows", "build_windows"}),
            ("hooks/pre-commit", {"lint", "git_hooks", "hook_inputs"}),
            ("hooks/sensitive-patterns.ini", {"git_hooks", "hook_inputs"}),
            (LINUX + "test-stocks.sh", {"stocks", "lint"}),
            (LINUX + "test-docs.sh", {"lint"}),
            (LINUX + "test-uninstall.sh", {"roundtrip_linux", "build_linux", "lint"}),
            (WINDOWS + "Test-ShellWrapper.ps1", {"wrapper_windows", "build_windows", "lint"}),
            (WINDOWS + "Test-InstallUninstall.ps1", {"roundtrip_windows", "build_windows", "lint"}),
            (WINDOWS + "Test-Applications.ps1", {"app_windows", "build_windows", "lint"}),
            (LINUX + "test-nvim-config.lua", {"app_tests", "build_linux"}),
            (LINUX + "test-shell-config.py", {"app_tests", "build_linux"}),
            (LINUX + "test-hook-inputs.sh", {"hook_inputs", "build_linux", "lint"}),
            (LINUX + "check-ci-contract.py", set()),
            (LINUX + "test-ci-changes.py", set()),
            (".github/workflows/release.yml", {"release"}),
            (LINUX + "classify-release.sh", {"release", "lint"}),
        ]
        for path, expected in cases:
            with self.subTest(path=path):
                outputs = classify([path])
                selected = {name.removeprefix("run_") for name in POLICY["FLAGS"] if outputs[name]}
                self.assertEqual(selected, expected | {"docs_checks"})

    def test_full_ci_and_empty_diff(self):
        for path in (".github/workflows/ci.yml", LINUX + "lib/test-helpers.sh",
                     LINUX + "classify-ci-changes.py", "unknown-build-input"):
            with self.subTest(path=path):
                outputs = classify([path])
                self.assertTrue(all(
                    outputs[name] for name in POLICY["FLAGS"]
                    if name not in {"run_release", "run_mutation"}
                ))
                self.assertFalse(outputs["run_mutation"])
        self.assertFalse(classify([".github/workflows/ci.yml"])["run_release"])
        self.assertTrue(classify(["unknown-build-input"])["run_release"])
        self.assertFalse(classify([], full=True)["run_release"])
        self.assertEqual(
            {name for name in POLICY["FLAGS"] if classify([])[name]}, {"run_docs_checks"}
        )

    def test_matrices_and_mixed_changes(self):
        cases = [
            (["dotfiles.sh"], ["ShellCheck"], []),
            (["dotfiles.ps1"], ["PSScriptAnalyzer"], []),
            (["dotfiles.sh", "dotfiles.ps1"], ["ShellCheck", "PSScriptAnalyzer"], []),
            (["symlinks/config/git/config"], [], ["git"]),
            (["symlinks/config/git/windows"], [], ["git", "zsh"]),
            (["symlinks/vim/init.vim"], [], ["vim", "nvim"]),
            ([LINUX + "test-nvim-config.lua"], [], ["nvim"]),
            ([LINUX + "test-shell-config.py"], [], ["zsh"]),
            (["symlinks/bashrc", "symlinks/config/wgetrc"], [], ["zsh"]),
            (["symlinks/config/zsh/prompt.zsh", "symlinks/config/git/config"],
             [], ["git", "zsh"]),
        ]
        for paths, linters, apps in cases:
            with self.subTest(paths=paths):
                outputs = classify(paths)
                self.assertEqual([item["name"] for item in outputs["lint_matrix"]["include"]], linters)
                self.assertEqual([item["application"] for item in outputs["app_matrix"]["include"]], apps)
                self.assertEqual(outputs["run_lint"], bool(linters))
                self.assertEqual(outputs["run_app_tests"], bool(apps))
        self.assertFalse(classify(["docs/README.md", "dotfiles.sh"])["docs_only"])
        self.assertTrue(classify(["docs/README.md", "AGENTS.md"])["docs_only"])

    def test_editor_matrices_include_plugin_free_regressions(self):
        matrix = classify(["symlinks/vim/vimrc"])["app_matrix"]["include"]
        self.assertEqual({entry["application"] for entry in matrix}, {"vim", "nvim"})
        for entry in matrix:
            self.assertIn("configuration", entry["tests"].split())

    def test_binary_inputs(self):
        for path in ("cli/src/main.rs", "cli/src/data.json", "cli/src/embedded.md", "cli/build.rs",
                     "cli/.cargo/config.toml", "cli/rust-toolchain.toml", "cli/Cargo.toml",
                     ".cargo/config.toml", "rust-toolchain.toml"):
            with self.subTest(path=path):
                self.assertTrue(classify([path])["run_release"])
        for path in ("cli/tests/install_command.rs", "cli/benches/fixture.json",
                     "cli/deny.toml", "cli/rustfmt.toml", "cli/README.md",
                     "dotfiles.sh", "conf/symlinks.toml", "symlinks/bashrc"):
            with self.subTest(path=path):
                self.assertFalse(classify([path])["run_release"])


class GateTests(unittest.TestCase):
    def results(self, paths):
        outputs = {name: str(value).lower() for name, value in classify(paths).items()}
        needs = {
            name: {"result": "success" if outputs[output] == "true" else "skipped"}
            for name, output in CONTRACT["JOB_CONDITIONS"].items()
            if name not in CONTRACT["INFORMATIONAL_JOBS"]
        }
        needs["classify-changes"] = {"result": "success", "outputs": outputs}
        return needs

    def test_intentional_skips(self):
        for paths in (["README.md"], ["dotfiles.ps1"], ["conf/packages.toml"], ["cli/src/main.rs"]):
            with self.subTest(paths=paths):
                CONTRACT["check_results"](self.results(paths))

    def test_failures_cancellations_and_unexpected_skips(self):
        for job, result in (
            ("classify-changes", "failure"), ("docs", "skipped"), ("docs", "cancelled"),
            ("docs", "failure"), ("build-linux", "failure"), ("build-linux", "success"),
        ):
            with self.subTest(job=job, result=result):
                needs = self.results(["README.md"])
                needs[job]["result"] = result
                with self.assertRaises(ValueError):
                    CONTRACT["check_results"](needs)
        needs = self.results(["README.md"])
        del needs["classify-changes"]["outputs"]["run_build_linux"]
        with self.assertRaises(ValueError):
            CONTRACT["check_results"](needs)

    def test_workflow_contracts(self):
        workflow = CONTRACT["WORKFLOW"].read_text(encoding="utf-8")
        CONTRACT["check"](workflow)
        release = CONTRACT["WORKFLOW"].with_name("release.yml").read_text(encoding="utf-8")
        CONTRACT["check_release"](release)
        for broken in (
            workflow.replace("      - docs\n", ""),
            workflow.replace("run_build_windows == 'true'", "run_build_linux == 'true'"),
            workflow.replace("check-ci-contract.py --results", "echo success"),
        ):
            with self.assertRaises(ValueError):
                CONTRACT["check"](broken)
        with self.assertRaises(ValueError):
            CONTRACT["check_release"](release.replace("run_release == 'true'", "true"))
        for platform in ("linux", "windows"):
            build = CONTRACT["job_blocks"](workflow)["build-" + platform]
            for step in ("Clippy", "Tests"):
                self.assertIn(
                    f"- name: {step}\n        if: needs.classify-changes.outputs.run_rust_checks == 'true'",
                    build,
                )


class GitRangeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dotfiles-ci-selection-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
            GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@test.local",
            GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@test.local",
            DIR=str(self.repo), GITHUB_OUTPUT=str(self.root / "outputs"),
            GITHUB_EVENT_NAME="push", GITHUB_REPOSITORY="fixture/repository",
        )
        self.git("init", "-q", "--template=")
        self.git("config", "core.hooksPath", os.devnull)
        self.git("config", "core.autocrlf", "false")
        for name in ("classify-release.sh", "classify-ci-changes.sh", "classify-ci-changes.py"):
            target = self.repo / LINUX / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SCRIPTS / name, target)
        self.write("cli/src/main.rs", "fn main() {}\n")
        self.base = self.commit()
        self.git("tag", "v2026.01.01-1")
        self.env["BASE_SHA"] = self.base
        self.env["HEAD_SHA"] = self.base

    def git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.repo), *args], env=self.env, check=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        ).stdout.strip()

    def write(self, path, text):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def script(self, name="classify-ci-changes.sh", *, success=True):
        output = Path(self.env["GITHUB_OUTPUT"])
        output.write_text("", encoding="utf-8")
        result = subprocess.run(
            ["sh", str(SCRIPTS / name)], env=self.env, cwd=self.repo,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        if not success:
            self.assertNotEqual(result.returncode, 0, result.stdout)
            self.assertNotIn("run_release=false", output.read_text(encoding="utf-8"))
            return {}
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return {
            key: json.loads(value)
            for key, value in (line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines())
        }

    def test_rename_deletion_and_unusual_paths(self):
        self.git("mv", "cli/src/main.rs", "example.md")
        self.env["HEAD_SHA"] = self.commit()
        outputs = self.script()
        for name in ("rust_checks", "build_linux", "build_windows", "profile_linux", "release"):
            self.assertTrue(outputs["run_" + name])
        self.assertFalse(outputs["docs_only"])
        self.env["BASE_SHA"] = self.env["HEAD_SHA"]
        self.write("docs/space and\nnewline.md", "docs\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["docs_only"])
        self.env["BASE_SHA"] = self.env["HEAD_SHA"]
        self.write("cli/src/space and\nnewline.rs", "fn fixture() {}\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["run_release"])
        self.env["BASE_SHA"] = self.env["HEAD_SHA"]
        self.git("rm", "cli/src/space and\nnewline.rs")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["run_release"])

    def test_empty_manual_missing_and_invalid_ranges(self):
        self.assertFalse(self.script()["run_build_linux"])
        self.env["GITHUB_EVENT_NAME"] = "workflow_dispatch"
        self.assertTrue(self.script()["run_build_windows"])
        self.env["GITHUB_EVENT_NAME"] = "push"
        self.env.pop("BASE_SHA")
        self.assertTrue(self.script()["run_build_windows"])
        self.env["BASE_SHA"] = "invalid-comparison"
        self.script(success=False)

    def mock_release(self, tag="v2026.01.01-1", status=0):
        mock = self.root / "bin"
        mock.mkdir(exist_ok=True)
        gh = mock / "gh"
        gh.write_text(
            '#!/bin/sh\n[ "$1" = release ] && [ "$2" = list ] || exit 90\n'
            'printf "%s\\n" "$FIXTURE_RELEASE_TAG"\nexit "$FIXTURE_GH_STATUS"\n',
            encoding="utf-8",
        )
        gh.chmod(0o755)
        self.env.update(
            PATH=str(mock) + os.pathsep + self.env["PATH"],
            FIXTURE_RELEASE_TAG=tag, FIXTURE_GH_STATUS=str(status),
        )

    def test_release_docs_skip_and_unpublished_binary_catch_up(self):
        self.mock_release()
        self.write("README.md", "docs\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertFalse(self.script("classify-release.sh")["run_release"])
        self.write("cli/src/main.rs", "fn main() { println!(\"updated\"); }\n")
        unpublished = self.commit()
        self.env["BASE_SHA"] = unpublished
        self.write("README.md", "more docs\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertFalse(self.script()["run_release"])
        self.assertTrue(self.script("classify-release.sh")["run_release"])
        self.git("tag", "v2026.01.01-2", unpublished)
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-2"
        self.assertFalse(self.script("classify-release.sh")["run_release"])

    def test_initial_release_rerun_stale_and_api_failure(self):
        self.mock_release(tag="")
        self.assertTrue(self.script("classify-release.sh")["run_release"])
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-1"
        self.assertFalse(self.script("classify-release.sh")["run_release"])
        self.write("cli/src/main.rs", "updated\n")
        newer = self.commit()
        self.git("tag", "v2026.01.01-2", newer)
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-2"
        self.assertFalse(self.script("classify-release.sh")["run_release"])
        self.env["FIXTURE_GH_STATUS"] = "1"
        self.script("classify-release.sh", success=False)

    def test_missing_tag_and_divergent_history_fail(self):
        self.mock_release(tag="v2026.01.01-99")
        self.script("classify-release.sh", success=False)
        self.git("checkout", "--orphan", "unrelated")
        self.git("commit", "-qm", "unrelated")
        self.env["HEAD_SHA"] = self.git("rev-parse", "HEAD")
        self.env["FIXTURE_RELEASE_TAG"] = "v2026.01.01-1"
        self.script("classify-release.sh", success=False)


if __name__ == "__main__":
    unittest.main()
