#!/usr/bin/env python3
"""Isolated regressions for CI selection, result gates, and release catch-up."""

import json
import os
from pathlib import Path
import re
import runpy
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent
POLICY = runpy.run_path(str(SCRIPTS / "classify-ci-changes.py"))
CONTRACT = runpy.run_path(str(SCRIPTS / "check-ci-contract.py"))
PRE_PUSH = runpy.run_path(str(SCRIPTS / "pre-push.py"))
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
            ("packages/apm/PKGBUILD", config | {"rust_checks", "lint"}),
            ("packages/apm/release.json", config | {"rust_checks", "lint"}),
            (".gitignore", set()),
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
            ("symlinks/config/hypr/hypridle.conf",
             config | {"managed_scripts", "lock"}),
            ("symlinks/config/quickshell/network_helper.py",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            ("symlinks/config/hypr/scripts/stocks.py",
             config | {"stocks", "managed_scripts", "lock"}),
            ("symlinks/config/hypr/scripts/tests/test_stocks.py",
             config | {"stocks", "managed_scripts", "lock"}),
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
            (LINUX + "check-doc-links.py", set()),
            (LINUX + "test-workflows.sh", {"lint"}),
            (LINUX + "test-desktop-qml.sh", {"lint", "desktop_qml"}),
            ("symlinks/config/quickshell/Bar.qml",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            ("symlinks/config/quickshell/Theme.js",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            ("symlinks/config/quickshell/tests/qml/mocks/Quickshell/Io/qmldir",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            (LINUX + "test-uninstall.sh", {"roundtrip_linux", "build_linux", "lint"}),
            (WINDOWS + "Test-ShellWrapper.ps1", {"wrapper_windows", "build_windows", "lint"}),
            (WINDOWS + "Test-InstallUninstall.ps1", {"roundtrip_windows", "build_windows", "lint"}),
            (WINDOWS + "Test-Applications.ps1", {"app_windows", "build_windows", "lint"}),
            ("symlinks/config/git/templates/hooks/post-rewrite",
             config | {"app_tests", "app_windows", "build_windows", "git_hooks"}),
            ("symlinks/config/systemd/user/clean-home-tmp.service", config | {"git_hooks"}),
            ("symlinks/config/systemd/user/clean-screenshots.service", config | {"git_hooks"}),
            (LINUX + "test-maintenance.py", {"git_hooks", "hook_inputs"}),
            (LINUX + "test-nvim-lock.lua", {"app_tests", "build_linux"}),
            (LINUX + "test-nvim-config.lua", {"app_tests", "build_linux"}),
            (LINUX + "test-shell-config.py", {"app_tests", "build_linux"}),
            (LINUX + "test-hook-inputs.sh", {"hook_inputs", "build_linux", "lint"}),
            (LINUX + "check-ci-contract.py", set()),
            (LINUX + "test-ci-changes.py", set()),
            (".github/workflows/release.yml", {"release"}),
            (LINUX + "classify-release.sh", {"release", "lint"}),
            (LINUX + "release-tag.sh", {"release", "lint"}),
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

    def test_shell_matrix_installs_extraction_and_preview_tools(self):
        matrix = classify(["symlinks/config/zsh/functions/extract"])["app_matrix"]["include"]
        self.assertEqual(len(matrix), 1)
        self.assertEqual(matrix[0]["application"], "zsh")
        self.assertIn("libarchive-tools", matrix[0]["packages"].split())
        self.assertIn("bat", matrix[0]["packages"].split())
        self.assertIn("zstd", matrix[0]["packages"].split())

    def test_binary_inputs(self):
        for path in ("cli/src/main.rs", "cli/src/data.json", "cli/src/embedded.md", "cli/build.rs",
                     "cli/.cargo/config.toml", "cli/rust-toolchain.toml", "cli/Cargo.toml",
                     ".cargo/config.toml", "rust-toolchain.toml"):
            with self.subTest(path=path):
                self.assertTrue(classify([path])["run_release"])
        for path in ("cli/tests/update_command.rs", "cli/benches/fixture.json",
                     "cli/deny.toml", "cli/rustfmt.toml", "cli/README.md",
                     "dotfiles.sh", "conf/symlinks.toml", "symlinks/bashrc",
                     "packages/apm/PKGBUILD", "packages/apm/release.json"):
            with self.subTest(path=path):
                self.assertFalse(classify([path])["run_release"])


class PrePushPlanTests(unittest.TestCase):
    def test_docs_stay_focused_and_rust_adds_platform_checks(self):
        commands, deferred = PRE_PUSH["plan"](classify(["README.md"]), "1.91")
        self.assertEqual(len(commands), 1)
        self.assertEqual(commands[0][2][-3:], ["--strict", "docs", "ci"])
        self.assertEqual(deferred, [])
        commands, deferred = PRE_PUSH["plan"](classify(["cli/src/main.rs"]), "1.91")
        flattened = [argument for _, _, command in commands for argument in command]
        for argument in ("test", "rustdoc", "x86_64-pc-windows-gnu", "1.91"):
            self.assertIn(argument, flattened)
        self.assertIn("mutation", deferred)
        self.assertIn("roundtrip_windows", deferred)
        self.assertFalse(any("update" in command or "remove" in command for _, _, command in commands))

    def test_config_and_desktop_select_their_regressions(self):
        commands, _ = PRE_PUSH["plan"](classify(["symlinks/config/quickshell/Bar.qml"]), "1.91")
        flattened = [argument for _, _, command in commands for argument in command]
        for argument in ("config_drift", "qml", "symlinks/config/quickshell/tests/python"):
            self.assertIn(argument, flattened)
        self.assertNotIn("clippy", flattened)

    def test_checker_changes_request_full_ci_without_binary_release(self):
        selected = classify([LINUX + "pre-push.py"])
        self.assertTrue(selected["run_rust_checks"])
        self.assertTrue(selected["run_hook_inputs"])
        self.assertTrue(selected["run_desktop_qml"])
        self.assertFalse(selected["run_release"])
        self.assertFalse(selected["run_mutation"])

    def test_missing_tools_fail_strict_checker(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            script = root / ".github/workflows/scripts/linux/check.sh"
            script.parent.mkdir(parents=True)
            (root / "cli").mkdir()
            shutil.copyfile(SCRIPTS / "check.sh", script)
            (root / "dirname").symlink_to(shutil.which("dirname"))
            for strict, expected in ((False, 0), (True, 1)):
                result = subprocess.run(
                    ["/bin/sh", str(script), *(["--strict"] if strict else []), "shell"],
                    env={"PATH": str(root)}, text=True, capture_output=True,
                )
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                self.assertIn("SKIP", result.stdout)
                self.assertNotIn("All requested checks passed", result.stdout)


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
        for paths in (["README.md"], ["dotfiles.ps1"], ["conf/packages.toml"], ["cli/src/main.rs"],
                      ["symlinks/config/quickshell/Bar.qml"], ["packages/apm/PKGBUILD"]):
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

    def test_qml_failures_and_skips_block_gate(self):
        for result in ("failure", "cancelled", "skipped"):
            with self.subTest(result=result):
                needs = self.results(["symlinks/config/quickshell/Bar.qml"])
                needs["desktop-qml"]["result"] = result
                with self.assertRaises(ValueError):
                    CONTRACT["check_results"](needs)

    def test_workflow_contracts(self):
        workflow = CONTRACT["WORKFLOW"].read_text(encoding="utf-8")
        CONTRACT["check"](workflow)
        release = CONTRACT["WORKFLOW"].with_name("release.yml").read_text(encoding="utf-8")
        CONTRACT["check_release"](release)
        for broken in (
            workflow.replace("      - docs\n", ""),
            workflow.replace("      - desktop-qml\n", ""),
            workflow.replace("run_build_windows == 'true'", "run_build_linux == 'true'"),
            workflow.replace("check-ci-contract.py --results", "echo success"),
        ):
            with self.assertRaises(ValueError):
                CONTRACT["check"](broken)
        with self.assertRaises(ValueError):
            CONTRACT["check_release"](release.replace("run_release == 'true'", "true"))
        for platform in ("linux", "windows"):
            build = CONTRACT["job_blocks"](workflow)["build-" + platform]
            for step in (("Clippy", "Tests", "Rustdoc") if platform == "linux" else ("Clippy", "Tests")):
                self.assertIn(
                    f"- name: {step}\n        if: needs.classify-changes.outputs.run_rust_checks == 'true'",
                    build,
                )

    def test_linux_mutation_excludes_only_named_windows_code(self):
        workflow = CONTRACT["WORKFLOW"].read_text(encoding="utf-8")
        mutation = CONTRACT["job_blocks"](workflow)["mutation"]
        self.assertIn("runs-on: ubuntu-latest", mutation)
        exclusions = [
            re.compile(pattern)
            for pattern in re.findall(r"--exclude-re '([^']+)'", mutation)
        ]
        self.assertTrue(exclusions)

        def excluded(path, description):
            name = f"src/{path}.rs:390:5: {description}"
            return any(pattern.search(name) for pattern in exclusions)

        windows_only = {
            "infra/elevation": ("run_elevated_child", "wait_if_elevated"),
            "domains/dotfiles/resources/path_entry": ("decode_reg_string",),
            "domains/system/resources/registry": (
                "native::raw_value_to_string", "native::raw_value_type",
                "native::read_value", "native::write_value", "native::parse_path",
            ),
        }
        for path, functions in windows_only.items():
            for function in functions:
                for description in (
                    f"replace {function} -> String with String::new()",
                    f'replace {function} -> String with "xyzzy".into()',
                    f"replace == with != in {function}",
                    f"replace && with || in {function}",
                ):
                    with self.subTest(path=path, description=description):
                        self.assertTrue(excluded(path, description))

        portable = {
            "infra/elevation": ("should_pause", "wait_if_elevated_policy"),
            "domains/dotfiles/resources/path_entry": (
                "PathSource::is_on_path", "PathEntryResource::new",
                "decode_reg_string_shared",
            ),
            "domains/system/resources/registry": (
                "parse_hkcu_subkey", "value_matches", "RegistryResource::state_from_cached",
            ),
            "another_module": ("decode_reg_string", "native::read_value", "wait_if_elevated"),
        }
        for path, functions in portable.items():
            for function in functions:
                for description in (
                    f"replace {function} -> bool with true",
                    f"replace == with != in {function}",
                ):
                    with self.subTest(path=path, description=description):
                        self.assertFalse(excluded(path, description))


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

    def test_pre_push_snapshot_excludes_local_fixes_and_preserves_modes(self):
        self.write("script.sh", "#!/bin/sh\nexit 1\n")
        (self.repo / "script.sh").chmod(0o755)
        head = self.commit()
        self.write("script.sh", "#!/bin/sh\nexit 0\n")
        self.write("untracked.txt", "not committed\n")
        destination = self.root / "snapshot"
        PRE_PUSH["snapshot"](self.repo, head, destination)
        self.assertEqual((destination / "script.sh").read_text(), "#!/bin/sh\nexit 1\n")
        self.assertFalse((destination / "untracked.txt").exists())
        if os.name != "nt":
            self.assertTrue(os.access(destination / "script.sh", os.X_OK))
        self.assertEqual(self.git("rev-parse", "HEAD"), head)
        self.assertEqual((self.repo / "script.sh").read_text(), "#!/bin/sh\nexit 0\n")

    def test_pre_push_uses_committed_failure_and_rejects_invalid_range(self):
        shutil.copyfile(SCRIPTS / "pre-push.py", self.repo / LINUX / "pre-push.py")
        self.write("cli/Cargo.toml", '[package]\nrust-version = "1.91"\n')
        self.write(LINUX + "check.sh", "#!/bin/sh\nexit 17\n")
        base = self.commit()
        self.write("README.md", "changed\n")
        head = self.commit()
        # The index is empty and the working-tree fix must not hide failure.
        self.write(LINUX + "check.sh", "#!/bin/sh\nexit 0\n")
        command = ["python3", "-B", str(self.repo / LINUX / "pre-push.py")]
        for arguments, expected, message in (
            (["--base", base, "--head", head], 1, "exit status 17"),
            (["--base", head, "--head", head], 0, "no checks run"),
            (["--base", "invalid-ref"], 1, "Pre-push validation failed"),
        ):
            result = subprocess.run(command + arguments, env=self.env, capture_output=True, text=True)
            self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
            self.assertIn(message, result.stdout + result.stderr)

    def test_pre_push_selection_ignores_dirty_or_missing_local_policy(self):
        shutil.copyfile(SCRIPTS / "pre-push.py", self.repo / LINUX / "pre-push.py")
        self.write("cli/Cargo.toml", '[package]\nrust-version = "1.91"\n')
        base = self.commit()
        self.write("cli/src/main.rs", "invalid rust\n")
        head = self.commit()
        policy = self.repo / LINUX / "classify-ci-changes.py"
        for content in ("raise RuntimeError('dirty policy executed')\n", None):
            with self.subTest(content=content):
                if content is None:
                    policy.unlink()
                else:
                    policy.write_text(content, encoding="utf-8")
                result = subprocess.run(
                    ["python3", "-B", str(self.repo / LINUX / "pre-push.py"),
                     "--plan", "--base", base, "--head", head],
                    env=self.env, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("clippy", result.stdout)
                self.assertIn("x86_64-pc-windows-gnu", result.stdout)

    def test_doc_links_preserve_paths_and_validate_angle_destinations(self):
        helper = runpy.run_path(str(SCRIPTS / "check-doc-links.py"))
        target = "docs/target with spaces.md"
        self.write(target, "# Heading\n# Heading\n```\n# Hidden\n```\n")
        self.write("docs/source with spaces.md", "\n".join((
            "[angle](<target with spaces.md#heading>)",
            "[encoded](target%20with%20spaces.md#heading-1)",
            "[external](<https://example.invalid/a b>)",
            "[mail](mailto:fixture%40example.invalid)",
        )))
        # A quoted Git display pathname must also be preserved on POSIX.
        if os.name != "nt":
            self.write("docs/tab\tand\nnewline.md", "[valid](<target with spaces.md>)\n")
        self.git("add", ".")
        self.assertEqual(helper["check"](self.repo), [])
        self.write("docs/source with spaces.md", "\n".join((
            "[missing](<missing with spaces.md>)",
            "[anchor](<target with spaces.md#absent>)",
            "[fence](<target with spaces.md#hidden>)",
        )))
        failures = helper["check"](self.repo)
        self.assertEqual(len(failures), 3, failures)
        self.assertIn("broken link", failures[0])
        self.assertIn("broken anchor", failures[1])
        # Archive/source snapshots without Git metadata use the same checks.
        archive = self.root / "archive"
        archive.mkdir()
        (archive / "source with spaces.md").write_text("[missing](<missing.md>)\n")
        self.assertEqual(len(helper["check"](archive)), 1)

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


class ReleaseTagTests(unittest.TestCase):
    def test_remote_presence_absence_and_errors(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            mock = root / "git"
            mock.write_text(
                '#!/bin/sh\n'
                'printf "%s\\n" "$5" >> "$TAG_LOG"\n'
                '[ "$TAG_STATUS" != sequence ] || {\n'
                '  [ "$5" = refs/tags/v2026.01.01-1 ] && exit 0\n'
                '  exit 2\n'
                '}\n'
                'exit "$TAG_STATUS"\n', encoding="utf-8",
            )
            mock.chmod(0o755)
            env = os.environ | {"PATH": str(root) + os.pathsep + os.environ["PATH"],
                                "TAG_LOG": str(root / "calls")}
            for status, mode, expected in (
                ("0", "check", 1), ("2", "check", 0),
                ("128", "check", 128), ("1", "check", 1),
                ("2", "next", 0), ("sequence", "next", 0),
                ("128", "next", 128), ("1", "next", 1),
            ):
                with self.subTest(status=status, mode=mode):
                    (root / "calls").write_text("")
                    result = subprocess.run(
                        ["sh", str(SCRIPTS / "release-tag.sh"), mode,
                         "2026.01.01" if mode == "next" else "v2026.01.01-1"],
                        env=env | {"TAG_STATUS": status}, capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                    if mode == "next" and expected == 0:
                        self.assertEqual(result.stdout.strip(), "v2026.01.01-" + ("2" if status == "sequence" else "1"))
                    if status in {"128", "1"}:
                        self.assertIn("Cannot query release tag", result.stderr)
                        self.assertEqual(len((root / "calls").read_text().splitlines()), 1)


if __name__ == "__main__":
    unittest.main()
