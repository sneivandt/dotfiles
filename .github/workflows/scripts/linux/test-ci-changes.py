#!/usr/bin/env python3
"""Isolated regressions for CI selection, result gates, and release catch-up."""

import contextlib
import io
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch


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
            (LINUX + "test-desktop-session.py", {"desktop_qml"}),
            ("docs/content/development/testing.md", {"docs_site", "docs_contracts"}),
            ("docs/.vitepress/config.mts", {"docs_site"}),
            ("docs/package-lock.json", {"docs_site"}),
            ("docs/.vitepress/theme/style.css", {"docs_site"}),
            (".github/workflows/pages.yml", {"docs_site"}),
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
            (LINUX + "test-docs.sh", {"lint", "docs_site", "docs_contracts"}),
            (LINUX + "check-doc-links.py", set()),
            (LINUX + "check-doc-site.py", {"docs_site"}),
            (LINUX + "docs-publication-base.py", {"docs_site"}),
            (LINUX + "test-workflows.sh", {"lint"}),
            (LINUX + "test-desktop-qml.sh", {"lint", "desktop_qml"}),
            ("symlinks/config/quickshell/Bar.qml",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            ("symlinks/config/quickshell/Theme.js",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            ("symlinks/config/quickshell/tests/qml/mocks/Quickshell/Io/qmldir",
             config | {"managed_scripts", "desktop_python", "desktop_qml"}),
            (LINUX + "test-arch-acceptance.sh", {"roundtrip_linux", "build_linux", "lint"}),
            (LINUX + "test-desktop-session.sh", {"desktop_qml", "lint"}),
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
                if path.startswith("cli/") and not path.endswith(".md"):
                    expected = expected | {"docs_contracts"}
                self.assertEqual(selected, expected | {"docs_checks"})

    def test_site_and_contract_inputs_are_independent(self):
        for path, site, contracts in (
            ("AGENTS.md", False, False),
            (".agents/skills/example/scripts/check.py", False, False),
            ("docs/content/reference/task-reference.md", True, True),
            ("docs/public/favicon.svg", True, False),
            ("docs/tests/browser/site.spec.ts", True, False),
            ("cli/src/embedded.md", False, True),
            ("cli/tests/docs_contracts.rs", False, True),
            (".cargo/config.toml", False, True),
            ("rust-toolchain.toml", False, True),
            ("conf/packages.toml", False, False),
        ):
            with self.subTest(path=path):
                selected = classify([path])
                self.assertEqual(selected["run_docs_site"], site)
                self.assertEqual(selected["run_docs_contracts"], contracts)
                self.assertTrue(selected["run_docs_checks"])

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
        self.assertFalse(classify(["docs/content/guides/getting-started.md", "dotfiles.sh"])["docs_only"])
        self.assertTrue(classify(["docs/content/guides/getting-started.md", "AGENTS.md"])["docs_only"])

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

    def test_mixed_selection_is_order_independent_and_never_drops_work(self):
        paths = [
            "cli/tests/config_drift.rs", "dotfiles.sh", "dotfiles.ps1",
            "symlinks/config/git/config", "symlinks/config/zsh/prompt.zsh",
            "symlinks/config/quickshell/Bar.qml", "README.md",
        ]
        expected = classify(paths)
        for ordered in (list(reversed(paths)), paths[3:] + paths[:3], paths + paths):
            with self.subTest(paths=ordered):
                self.assertEqual(classify(ordered), expected)
        for flag in POLICY["FLAGS"]:
            self.assertEqual(
                expected[flag], any(classify([path])[flag] for path in paths), flag,
            )
        self.assertEqual(
            [entry["application"] for entry in expected["app_matrix"]["include"]],
            ["git", "zsh"],
        )
        self.assertEqual(
            [entry["name"] for entry in expected["lint_matrix"]["include"]],
            ["ShellCheck", "PSScriptAnalyzer"],
        )
        self.assertFalse(expected["docs_only"])

    def test_embedded_documentation_and_similar_prefixes_are_not_docs_only(self):
        for path in ("cli/src/README.md", "cli/src/docs/guide.md", "docs-build/input",
                     ".agents-build/input", "cli/tests-extra/input"):
            with self.subTest(path=path):
                outputs = classify([path])
                self.assertFalse(outputs["docs_only"])
                self.assertTrue(outputs["run_release"])
                self.assertTrue(outputs["run_build_linux"])
                self.assertTrue(outputs["run_build_windows"])


class PrePushPlanTests(unittest.TestCase):
    def test_native_docs_reuse_keeps_all_implementation_checks(self):
        selection = classify(["cli/Cargo.toml"])
        commands, deferred = PRE_PUSH["plan"](selection, "1.91")
        reused, remaining = PRE_PUSH["plan"](selection, "1.91", docs_checked=True)
        expected = [
            (label, directory, [argument for argument in command if argument != "docs"])
            for label, directory, command in commands if label != "documentation dependencies"
        ]
        self.assertEqual(reused, expected)
        self.assertEqual(remaining, deferred)

    def test_docs_stay_focused_and_rust_adds_platform_checks(self):
        commands, deferred = PRE_PUSH["plan"](classify(["README.md"]), "1.91")
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[0][2], ["npm", "ci", "--no-audit", "--no-fund"])
        self.assertEqual(commands[1][2][-3:], ["--strict", "docs", "ci"])
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

    def test_application_fixtures_follow_selected_applications(self):
        for path, expected, absent in (
            ("symlinks/zshrc", "test-shell-config.py", "symlinks/vim/tests"),
            ("symlinks/vim/vimrc", "symlinks/vim/tests", "test-shell-config.py"),
        ):
            commands, _ = PRE_PUSH["plan"](classify([path]), "1.91")
            flattened = " ".join(arg for _, _, command in commands for arg in command)
            self.assertIn(expected, flattened)
            self.assertNotIn(absent, flattened)

    def test_checker_changes_request_full_ci_without_binary_release(self):
        selected = classify([LINUX + "pre-push.py"])
        self.assertTrue(selected["run_rust_checks"])
        self.assertTrue(selected["run_hook_inputs"])
        self.assertTrue(selected["run_desktop_qml"])
        self.assertFalse(selected["run_release"])
        self.assertFalse(selected["run_mutation"])

    @unittest.skipUnless(sys.platform == "linux", "requires POSIX fixture executables")
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


class PrePushCoordinatorTests(unittest.TestCase):
    def test_native_docs_only_do_not_run_rust_or_wrappers(self):
        commands = PRE_PUSH["windows_plan"](classify(["README.md"]))
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[0][2], ["npm.cmd", "ci", "--no-audit", "--no-fund"])
        self.assertEqual(commands[1][2], [sys.executable, "-B", "docs/check.py"])
        self.assertEqual(PRE_PUSH["windows_plan"](classify(["README.md"]), docs_checked=True), [])

    def test_native_windows_covers_runtime_without_repeating_linux_only_checks(self):
        selection = classify(["cli/src/main.rs", "dotfiles.ps1"])
        commands = PRE_PUSH["windows_plan"](selection)
        stages = next(command for label, _, command in commands if label == "native Windows checks")
        self.assertEqual(stages[stages.index("-Strict") + 1:], ["clippy", "test", "config", "powershell"])
        flattened = [arg for _, _, command in commands for arg in command]
        for unnecessary in ("fmt", "rustdoc", "audit", "deny", "x86_64-pc-windows-gnu"):
            self.assertNotIn(unnecessary, flattened)
        wrapper = next(command for label, _, command in commands if label == "isolated PowerShell wrapper")
        self.assertIn("if (-not (Test-IsolatedWrapperPath)) { exit 1 }", wrapper[-1])
        self.assertIn("if (-not (Test-CargoArtifactPath)) { exit 1 }", wrapper[-1])

    def test_windows_receipt_omits_only_redundant_compilation_and_analyzer(self):
        selection = classify(["cli/src/main.rs", "dotfiles.ps1"])
        original, deferred = PRE_PUSH["plan"](selection, "1.91", docs_checked=True)
        reused, remaining = PRE_PUSH["plan"](selection, "1.91", docs_checked=True, windows_checked=True)
        expected = [
            (label, directory, [arg for arg in command if arg != "powershell"])
            if label == "core checks" else (label, directory, command)
            for label, directory, command in original
            if label != "Windows compilation (no runtime coverage)"
        ]
        self.assertEqual(reused, expected)
        self.assertEqual(remaining, deferred)
        self.assertTrue(any("test" in command for _, _, command in reused))

    def test_wsl_worker_uses_exact_range_and_receipts(self):
        for unavailable in (False, True):
            with self.subTest(unavailable=unavailable), patch("shutil.which", return_value="wsl"), patch(
                "subprocess.check_output", return_value="/mnt/c/repo with spaces/pre-push.py\n",
            ) as output:
                command = PRE_PUSH["wsl_command"]("base-sha", "head-sha", True, npm_tls_unavailable=unavailable)
            self.assertEqual(command[:5], ["wsl", "--exec", "python3", "-B", "/mnt/c/repo with spaces/pre-push.py"])
            receipt = "--npm-tls-unavailable" if unavailable else "--docs-checked"
            self.assertEqual(command[5:], ["--base", "base-sha", "--head", "head-sha",
                                         receipt, "head-sha", "--windows-checked", "head-sha", "--full"])
            self.assertEqual(output.call_args.args[0][:4], ["wsl", "--exec", "wslpath", "-u"])
        with patch("shutil.which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "WSL is unavailable"):
                PRE_PUSH["wsl_command"]("base", "head", False)

    def test_wsl_cache_is_not_shared_with_windows(self):
        root = Path(tempfile.gettempdir()) / "mounted source"
        with patch.object(sys, "platform", "linux"), patch.dict(os.environ, {"WSL_INTEROP": "fixture"}):
            cache = PRE_PUSH["target_cache"](root)
            self.assertTrue(cache.is_relative_to(Path.home() / ".cache/dotfiles-pre-push"))
            self.assertNotEqual(cache, root / "cli/target")
            self.assertEqual(cache, PRE_PUSH["target_cache"](root))
        with patch.object(sys, "platform", "win32"):
            self.assertEqual(PRE_PUSH["target_cache"](root), root / "cli/target")

    def test_explicit_baseline_does_not_query_github(self):
        with patch.dict(PRE_PUSH["comparison_base"].__globals__, {"git": lambda *args: "original-sha"}):
            with patch("subprocess.check_output") as output:
                self.assertEqual(PRE_PUSH["comparison_base"](".", "original", "head"), ("original-sha", False))
                output.assert_not_called()

    def test_default_baseline_recovers_pending_source_changes(self):
        for remote in ("https://github.com/fixture/repository.git", "git@github.com:fixture/repository.git"):
            for conclusion in ("success", "failure", "cancelled", None):
                with self.subTest(remote=remote, conclusion=conclusion):
                    outputs = [json.dumps({"workflow_runs": [{"conclusion": conclusion}]})]
                    if conclusion != "success":
                        outputs.append(json.dumps({"workflow_runs": [{"head_sha": "original-sha"}]}))
                    with patch.dict(PRE_PUSH["comparison_base"].__globals__, {
                        "git": lambda root, *args: remote if args[0] == "remote" else "tip-sha",
                    }), patch("subprocess.check_output", side_effect=outputs) as output, patch("subprocess.run") as run:
                        run.return_value.returncode = 0
                        expected = ("tip-sha", False) if conclusion == "success" else ("original-sha", True)
                        self.assertEqual(PRE_PUSH["comparison_base"](".", None, "head"), expected)
                        self.assertIn("repos/fixture/repository/actions/workflows/ci.yml/runs", output.call_args.args[0])

    def test_missing_or_unverifiable_baseline_fails_closed(self):
        with patch.dict(PRE_PUSH["comparison_base"].__globals__, {
            "git": lambda root, *args: "https://github.com/fixture/repository.git" if args[0] == "remote" else "tip",
        }):
            with patch("subprocess.check_output", return_value='{"workflow_runs": []}'):
                with self.assertRaisesRegex(RuntimeError, "No successful CI ancestor"):
                    PRE_PUSH["comparison_base"](".", None, "head")
            with patch("subprocess.check_output", side_effect=subprocess.CalledProcessError(1, ["gh"])):
                with self.assertRaises(subprocess.CalledProcessError):
                    PRE_PUSH["comparison_base"](".", None, "head")

    def test_windows_runs_native_before_linux_and_never_continues_after_failure(self):
        worker = ["fixture-wsl", "--head", "head-sha"]
        for failure in (False, True):
            events = []

            def execute(*args):
                events.append("native")
                if failure:
                    raise subprocess.CalledProcessError(17, ["fixture-native"])

            def run(command, **kwargs):
                if command == worker:
                    events.append("linux")

            replacements = {
                "git": lambda root, *args: str(SCRIPTS.parents[3]) if "--show-toplevel" in args else "head-sha",
                "comparison_base": lambda *args: ("base-sha", True),
                "committed_policy": lambda *args: {
                    "changed_paths": lambda *args: ["cli/src/main.rs"], "classify": classify,
                },
                "execute": execute,
                "wsl_command": Mock(return_value=worker),
            }
            # main reads only the committed Cargo rust-version after selection.
            real_git = replacements["git"]
            replacements["git"] = lambda root, *args: (
                '[package]\nrust-version = "1.91"\n' if args[0] == "show" else real_git(root, *args)
            )
            with patch.dict(PRE_PUSH["main"].__globals__, replacements), patch.object(sys, "platform", "win32"), patch.object(
                sys, "argv", ["pre-push.py"],
            ), patch("subprocess.run", side_effect=run), contextlib.redirect_stdout(io.StringIO()):
                if failure:
                    with self.assertRaises(subprocess.CalledProcessError):
                        PRE_PUSH["main"]()
                else:
                    PRE_PUSH["main"]()
            self.assertEqual(events, ["native"] if failure else ["native", "linux"])
            replacements["wsl_command"].assert_called_once_with("base-sha", "head-sha", True, npm_tls_unavailable=False)

    def test_tls_receipt_defers_only_docs_and_never_hides_other_failures(self):
        for platform in ("win32", "linux"):
            for failure in (False, True):
                with self.subTest(platform=platform, failure=failure):
                    execute = Mock(side_effect=subprocess.CalledProcessError(17, ["fixture-check"]) if failure else None)
                    worker = ["fixture-wsl"]
                    replacements = {
                        "git": lambda root, *args: (
                            '[package]\nrust-version = "1.91"\n' if args[0] == "show" else "head-sha"
                        ),
                        "comparison_base": lambda *args: ("base-sha", False),
                        "committed_policy": lambda *args: {
                            "changed_paths": lambda *args: ["conf/symlinks.toml"], "classify": classify,
                        },
                        "execute": execute,
                        "wsl_command": Mock(return_value=worker),
                    }
                    output = io.StringIO()
                    with patch.dict(PRE_PUSH["main"].__globals__, replacements), patch.object(sys, "platform", platform), patch.object(
                        sys, "argv", ["pre-push.py", "--npm-tls-unavailable", "head-sha"],
                    ), patch("subprocess.run") as run, contextlib.redirect_stdout(output):
                        if failure:
                            with self.assertRaises(subprocess.CalledProcessError):
                                PRE_PUSH["main"]()
                        else:
                            PRE_PUSH["main"]()
                    commands = execute.call_args.args[2]
                    self.assertTrue(commands)
                    self.assertFalse(any("documentation" in label for label, _, _ in commands))
                    self.assertNotIn("docs", [arg for _, _, command in commands for arg in command])
                    self.assertIn("config", [arg for _, _, command in commands for arg in command])
                    self.assertIn("UNAVAILABLE documentation for head-sha", output.getvalue())
                    self.assertNotIn("Selected local checks passed", output.getvalue())
                    self.assertEqual("Local checks completed" in output.getvalue(), not failure)
                    worker_calls = [call for call in run.call_args_list if call.args[0] == worker]
                    self.assertEqual(len(worker_calls), int(platform == "win32" and not failure))
                    if platform == "win32":
                        replacements["wsl_command"].assert_called_once_with(
                            "base-sha", "head-sha", False, npm_tls_unavailable=True,
                        )

    def test_success_and_tls_unavailability_receipts_are_mutually_exclusive(self):
        with patch.object(sys, "argv", [
            "pre-push.py", "--docs-checked", "head", "--npm-tls-unavailable", "head",
        ]), contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                PRE_PUSH["main"]()
        self.assertEqual(error.exception.code, 2)

    def test_mismatched_receipt_fails_before_executing_checks(self):
        replacements = {
            "git": lambda root, *args: "other-sha" if args[-1] == "old^{commit}" else "head-sha",
            "comparison_base": lambda *args: ("base-sha", False),
            "execute": Mock(),
        }
        for option in ("--docs-checked", "--windows-checked", "--npm-tls-unavailable"):
            with patch.dict(PRE_PUSH["main"].__globals__, replacements), patch.object(sys, "platform", "linux"), patch.object(
                sys, "argv", ["pre-push.py", option, "old"],
            ):
                with self.assertRaisesRegex(RuntimeError, "exact commit"):
                    PRE_PUSH["main"]()
        replacements["execute"].assert_not_called()


class LocalCheckTests(unittest.TestCase):
    def test_tool_failures_are_not_optional_tool_skips(self):
        stages = (
            "fmt", "clippy", "test", "rustdoc", "config", "docs", "ci",
            "shell", "powershell", "audit", "deny", "msrv", "qml",
        )
        with tempfile.TemporaryDirectory(prefix="dotfiles-check-status-") as temporary:
            root = Path(temporary)
            scripts = root / LINUX
            scripts.mkdir(parents=True)
            (root / "cli").mkdir()
            (root / "cli/Cargo.toml").write_text(
                '[package]\nrust-version = "1.91"\n', encoding="utf-8", newline="\n",
            )
            (root / "hooks").mkdir()
            (root / "hooks/check-toolchain.sh").write_text(
                "#!/bin/sh\nexit 0\n", encoding="utf-8", newline="\n",
            )
            shutil.copyfile(SCRIPTS / "check.sh", scripts / "check.sh")
            for name in (
                "test-docs.sh", "test-static-analysis.sh", "test-workflows.sh",
                "test-desktop-qml.sh",
            ):
                (scripts / name).write_text(
                    '#!/bin/sh\nexit "$CHECK_STATUS"\n', encoding="utf-8", newline="\n",
                )
            tools = root / "tools"
            tools.mkdir()
            for name in ("cargo", "rustup", "python3", "shellcheck", "pwsh",
                         "actionlint", "qmltestrunner"):
                tool = tools / name
                tool.write_text(
                    '#!/bin/sh\n'
                    '[ "${2:-}" = --version ] && exit 0\n'
                    '[ "${1:-}" = toolchain ] && exit 0\n'
                    'exit "$CHECK_STATUS"\n',
                    encoding="utf-8", newline="\n",
                )
                tool.chmod(0o755)
            env = os.environ | {
                "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                "DOTFILES_CHECK_STRICT": "0",
            }
            for status in (0, 1, 2, 17):
                with self.subTest(status=status):
                    result = subprocess.run(
                        ["sh", str(scripts / "check.sh"), *stages],
                        env=env | {"CHECK_STATUS": str(status)},
                        capture_output=True, text=True, timeout=120,
                    )
                    self.assertEqual(result.returncode, int(status != 0), result.stdout + result.stderr)
                    self.assertNotIn("SKIP", result.stdout)
                    for stage in stages:
                        self.assertRegex(
                            result.stdout,
                            rf"(?m)^\s+{'PASS' if status == 0 else 'FAIL'} {stage}$",
                        )

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell is unavailable")
    def test_windows_command_start_failures_do_not_pass(self):
        command = r"""
$ErrorActionPreference = 'Continue'
$source = [System.Management.Automation.Language.Parser]::ParseFile(
    $env:CHECK_SCRIPT, [ref]$null, [ref]$null)
foreach ($name in @('Write-Note', 'Invoke-Tool')) {
    $function = $source.Find({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq $name
    }, $true)
    Invoke-Expression $function.Extent.Text
}
$global:LASTEXITCODE = 0
$status = Invoke-Tool -FilePath 'dotfiles-fixture-command-that-does-not-exist' -ToolArguments @('--version')
if ($status -ne 'FAIL') { throw "Expected FAIL, got $status" }
"""
        result = subprocess.run(
            [shutil.which("pwsh"), "-NoProfile", "-NonInteractive", "-Command", command],
            env=os.environ | {"CHECK_SCRIPT": str(SCRIPTS.parent / "windows/Check.ps1")},
            capture_output=True, text=True, timeout=120,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell is unavailable")
    def test_windows_skips_are_reported_and_strict_mode_fails(self):
        with tempfile.TemporaryDirectory(prefix="dotfiles-check-windows-") as temporary:
            root = Path(temporary)
            script = root / WINDOWS / "Check.ps1"
            script.parent.mkdir(parents=True)
            (root / "cli").mkdir()
            shutil.copyfile(SCRIPTS.parent / "windows/Check.ps1", script)
            env = os.environ | {"PATH": "", "DOTFILES_CHECK_STRICT": "0"}
            for strict, inherited in ((False, False), (True, False), (False, True)):
                with self.subTest(strict=strict, inherited=inherited):
                    result = subprocess.run(
                        [shutil.which("pwsh"), "-NoProfile", "-NonInteractive", "-File", str(script),
                         *(["-Strict"] if strict else []), "fmt"],
                        env=env | {"DOTFILES_CHECK_STRICT": "1" if inherited else "0"},
                        capture_output=True, text=True, timeout=120,
                    )
                    self.assertEqual(
                        result.returncode, int(strict or inherited),
                        result.stdout + result.stderr,
                    )
                    self.assertIn("SKIP fmt", result.stdout)
                    self.assertNotIn("All checks passed", result.stdout)
                    if not (strict or inherited):
                        self.assertIn("Some requested checks were skipped", result.stdout)


class GateTests(unittest.TestCase):
    def test_scheduled_security_checks_are_independent_and_read_only(self):
        workflow = CONTRACT["WORKFLOW"].with_name("dependency-security.yml").read_text(encoding="utf-8")
        self.assertRegex(workflow, r"(?m)^  schedule:$")
        self.assertRegex(workflow, r"(?m)^    - cron: '.+'$")
        self.assertIn("  workflow_dispatch:", workflow)
        self.assertIn("permissions:\n  contents: read\n", workflow)
        self.assertNotRegex(workflow, r"(?m)^\s+(?:paths|paths-ignore):")
        for block in CONTRACT["job_blocks"](workflow).values():
            self.assertIsNone(CONTRACT["job_condition"](block))
            self.assertFalse(CONTRACT["job_needs"](block))
        self.assertIn("run: cargo audit", workflow)
        self.assertIn("if: ${{ !cancelled() }}\n        run: cargo deny check all", workflow)
        self.assertNotIn("continue-on-error:", workflow)
        self.assertNotIn("write", workflow)

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
                needs = self.results(["docs/content/development/testing.md"])
                needs[job]["result"] = result
                with self.assertRaises(ValueError):
                    CONTRACT["check_results"](needs)
        needs = self.results(["README.md"])
        del needs["classify-changes"]["outputs"]["run_build_linux"]
        with self.assertRaises(ValueError):
            CONTRACT["check_results"](needs)

    def test_mutation_must_succeed_when_selected(self):
        for result in ("failure", "cancelled", "skipped"):
            with self.subTest(result=result):
                needs = self.results(["cli/src/main.rs"])
                needs["mutation"]["result"] = result
                with self.assertRaisesRegex(ValueError, f"mutation: expected success, got {result}"):
                    CONTRACT["check_results"](needs)

    def test_every_selected_job_must_report_success(self):
        paths = [".github/workflows/ci.yml", "cli/src/main.rs"]
        for job in self.results(paths):
            for result in ("failure", "cancelled", "skipped"):
                with self.subTest(job=job, result=result):
                    needs = self.results(paths)
                    needs[job]["result"] = result
                    with self.assertRaises(ValueError):
                        CONTRACT["check_results"](needs)

    def test_missing_jobs_and_malformed_selection_outputs_fail_closed(self):
        baseline = self.results(["docs/content/development/testing.md"])
        for job in baseline:
            with self.subTest(missing_job=job):
                needs = self.results(["README.md"])
                del needs[job]
                with self.assertRaises(ValueError):
                    CONTRACT["check_results"](needs)
        gated_outputs = {
            output for job, output in CONTRACT["JOB_CONDITIONS"].items()
            if job not in CONTRACT["INFORMATIONAL_JOBS"]
        }
        for output in sorted(gated_outputs):
            for value in (None, "", "TRUE", "false\n", True):
                with self.subTest(output=output, value=value):
                    needs = self.results(["README.md"])
                    needs["classify-changes"]["outputs"][output] = value
                    with self.assertRaises(ValueError):
                        CONTRACT["check_results"](needs)

    def test_workflow_contracts(self):
        workflow = CONTRACT["WORKFLOW"].read_text(encoding="utf-8")
        CONTRACT["check"](workflow)
        release = CONTRACT["WORKFLOW"].with_name("release.yml").read_text(encoding="utf-8")
        CONTRACT["check_release"](release)
        pages = CONTRACT["WORKFLOW"].with_name("pages.yml").read_text(encoding="utf-8")
        CONTRACT["check_pages"](pages, workflow)
        for broken in (
            pages.replace("github.event.workflow_run.event == 'push'", "true"),
            pages.replace("github.event.workflow_run.head_repository.full_name == github.repository", "true"),
            pages.replace("run-id: ${{ github.event.workflow_run.id }}", "run-id: 1"),
            pages.replace('select(.name == "Docs: Content and site")', 'select(.name == "Other job")'),
            pages.replace("if: steps.current.outputs.current == 'true'", "if: always()", 1),
        ):
            with self.assertRaises(ValueError):
                CONTRACT["check_pages"](broken, workflow)
        for broken in (
            workflow.replace("      - docs\n", ""),
            workflow.replace("test-docs.sh docs_links", "true"),
            workflow.replace("test-docs.sh docs_task_selectors", "true"),
            workflow.replace("DOCS_BASE_SHA:", "UNUSED_BASE_SHA:"),
            workflow.replace("      - mutation\n", ""),
            workflow.replace("  mutation:\n", "  mutation:\n    continue-on-error: true\n"),
            workflow.replace("      - name: Test changed-code mutants\n",
                             "      - name: Test changed-code mutants\n        continue-on-error: true\n"),
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

    def test_linux_mutation_excludes_only_native_backends_and_equivalent_guard(self):
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
            "domains/dotfiles/resources/path_entry": (
                "decode_reg_string", "append_user_path", "user_path_contains",
            ),
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
                "decode_reg_string_shared", "path_contains_entry", "expand_env_vars",
                "normalize_path_entry", "append_user_path_shared", "user_path_contains_shared",
            ),
            "domains/ai/resources/agent_settings": ("AgentSettingResource::apply",),
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

        settings = "domains/ai/resources/agent_settings"
        equivalent = (
            "replace match guard metadata.is_symlink() with true "
            "in <impl Resource for AgentSettingResource>::apply"
        )
        self.assertTrue(excluded(settings, equivalent))
        for path, description in (
            (settings, equivalent.replace("with true", "with false")),
            (settings, equivalent.replace("metadata.is_symlink()", "value_is_finite()")),
            (settings, equivalent.replace("::apply", "::current_state")),
            ("another_module", equivalent),
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
        home = self.root / "home"
        home.mkdir()
        gitconfig = self.root / "fixture-gitconfig"
        gitconfig.write_text(
            "[core]\n\tfsmonitor = false\n"
            "[maintenance]\n\tauto = false\n"
            "[gc]\n\tauto = 0\n",
            encoding="utf-8",
        )
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(
            HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
            GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(gitconfig), GIT_CONFIG_COUNT="0",
            GIT_CEILING_DIRECTORIES=str(self.root),
            GIT_AUTHOR_NAME="Fixture", GIT_AUTHOR_EMAIL="fixture@test.local",
            GIT_COMMITTER_NAME="Fixture", GIT_COMMITTER_EMAIL="fixture@test.local",
            DIR=str(self.repo), GITHUB_OUTPUT=str(self.root / "outputs"),
            GITHUB_EVENT_NAME="push", GITHUB_REPOSITORY="fixture/repository",
        )
        # Imported helpers also spawn Git, without receiving self.env explicitly.
        environment = patch.dict(os.environ, self.env, clear=True)
        environment.start()
        self.addCleanup(environment.stop)
        # TMPDIR may be inside a checkout. Neither archive checks nor Git
        # initialization should discover that enclosing repository.
        working_directory = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, working_directory)
        self.git("init", "-q", "--template=", "--initial-branch=fixture")
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
        git_dir = self.repo / ".git"
        env = self.env | {
            "GIT_DIR": str(git_dir),
            "GIT_COMMON_DIR": str(git_dir),
            "GIT_WORK_TREE": str(self.repo),
            "GIT_INDEX_FILE": str(git_dir / "index"),
        }
        result = subprocess.run(
            ["git", *args], cwd=self.repo, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        self.assertEqual(result.returncode, 0, f"git {args!r}: {result.stderr}")
        return result.stdout.strip()

    def write(self, path, text):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def test_imported_helpers_use_the_isolated_git_environment(self):
        for key in ("GIT_CONFIG_PARAMETERS", "GIT_CONFIG_KEY_0",
                    "GIT_CONFIG_VALUE_0", "GIT_EXEC_PATH"):
            self.assertNotIn(key, os.environ)
        self.assertEqual(os.environ["GIT_CONFIG_GLOBAL"], self.env["GIT_CONFIG_GLOBAL"])
        self.assertEqual(
            PRE_PUSH["git"](self.repo, "config", "--global", "--get", "maintenance.auto"),
            "false",
        )
        self.assertEqual(
            Path(PRE_PUSH["git"](self.repo, "rev-parse", "--show-toplevel")),
            self.repo,
        )

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
            self.assertEqual(output.read_text(encoding="utf-8"), "")
            return {}
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return {
            key: json.loads(value)
            for key, value in (line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines())
        }

    def test_docs_push_carries_forward_unverified_rust_changes(self):
        self.write("cli/src/main.rs", "fn main() { println!(\"changed\"); }\n")
        before = self.commit()
        self.write("docs/example.md", "docs only\n")
        head = self.commit()

        tools = self.root / "tools"
        tools.mkdir()
        gh = tools / "gh"
        gh.write_text(
            '#!/bin/sh\n'
            'test "$1" = api && test "$4" = "repos/fixture/repository/actions/workflows/ci.yml/runs" || exit 2\n'
            'case "${10}" in\n'
            '  "head_sha=$PUSH_BEFORE") if [ "$FIXTURE_CI" = success ]; then echo "$PUSH_BEFORE"; fi;;\n'
            '  status=success)\n'
            '    if [ "$FIXTURE_CI" = unavailable ]; then exit 1; fi\n'
            '    printf "%s\\n" "$GITHUB_SHA" "$FIXTURE_BASE";;\n'
            '  *) exit 2;;\n'
            'esac\n',
            encoding="utf-8",
        )
        gh.chmod(0o755)
        self.env.update(
            PATH=str(tools) + os.pathsep + self.env["PATH"],
            PUSH_BEFORE=before, GITHUB_SHA=head, FIXTURE_BASE=self.base,
        )
        for state in ("success", "cancelled", "failed", "in_progress", "unavailable", "initial"):
            with self.subTest(state=state):
                self.env.update(FIXTURE_CI=state, FIXTURE_BASE="" if state == "initial" else self.base)
                if state == "unavailable":
                    self.script("ci-comparison-range.sh", success=False)
                    continue
                output = Path(self.env["GITHUB_OUTPUT"])
                output.write_text("", encoding="utf-8")
                result = subprocess.run(
                    ["sh", str(SCRIPTS / "ci-comparison-range.sh")],
                    env=self.env, cwd=self.repo, check=True, capture_output=True, text=True,
                )
                values = dict(
                    line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines()
                )
                expected = {"success": before, "initial": self.git("hash-object", "-t", "tree", "/dev/null")}.get(state, self.base)
                self.assertEqual((values["base_sha"], values["head_sha"]), (expected, head))
                self.env.update(BASE_SHA=values["base_sha"], HEAD_SHA=values["head_sha"],
                                CI_FORCE_FULL=values["force_full"])
                jobs = self.script()
                self.assertEqual(jobs["docs_only"], state == "success", result.stderr)
                self.assertEqual(jobs["run_mutation"], state != "success")
                self.assertEqual(jobs["run_build_linux"], state != "success")
                self.assertEqual(jobs["run_build_windows"], state != "success")

    def test_manual_recovery_uses_the_requested_mutation_range(self):
        self.write("cli/src/main.rs", 'fn main() { println!("changed"); }\n')
        head = self.commit()
        self.env.update(GITHUB_EVENT_NAME="workflow_dispatch", GITHUB_SHA=head,
                        MUTATION_BASE=self.base)
        output = Path(self.env["GITHUB_OUTPUT"])
        output.write_text("", encoding="utf-8")
        subprocess.run(
            ["sh", str(SCRIPTS / "ci-comparison-range.sh")],
            env=self.env, cwd=self.repo, check=True, capture_output=True, text=True,
        )
        values = dict(
            line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines()
        )
        self.assertEqual((values["base_sha"], values["head_sha"]), (self.base, head))
        self.assertEqual(values["force_full"], "true")
        self.env.update(BASE_SHA=values["base_sha"], HEAD_SHA=values["head_sha"],
                        CI_FORCE_FULL=values["force_full"])
        jobs = self.script()
        self.assertTrue(jobs["run_mutation"])
        self.assertTrue(jobs["run_build_windows"])

        self.env["MUTATION_BASE"] = "HEAD"
        self.script("ci-comparison-range.sh", success=False)

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
        self.write("docs/package.json", '{"private":true}\n')
        self.write(LINUX + "check.sh", "#!/bin/sh\nexit 17\n")
        tools = self.root / "tools"
        tools.mkdir()
        npm = tools / "npm"
        npm.write_text('#!/bin/sh\n[ "$*" = "ci --no-audit --no-fund" ] || exit 92\n')
        npm.chmod(0o755)
        self.env["PATH"] = str(tools) + os.pathsep + self.env["PATH"]
        base = self.commit()
        self.write("README.md", "changed\n")
        head = self.commit()
        # The index is empty and the working-tree fix must not hide failure.
        self.write(LINUX + "check.sh", "#!/bin/sh\nexit 0\n")
        command = ["python3", "-B", str(self.repo / LINUX / "pre-push.py")]
        for arguments, expected, message in (
            (["--base", base, "--head", head], 1, "exit status 17"),
            (["--base", base, "--head", head, "--docs-checked", head], 1, "exit status 17"),
            (["--base", base, "--head", head, "--docs-checked", base], 1, "exact commit"),
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

    def test_doc_links_follow_unstaged_moves_and_ignore_generated_files(self):
        helper = runpy.run_path(str(SCRIPTS / "check-doc-links.py"))
        self.write("docs/guide.md", "# Guide\n")
        self.git("add", ".")
        (self.repo / "docs/guide.md").unlink()
        self.write("docs/content/guide.md", "[missing](missing.md)\n")
        self.write(".gitignore", "docs/node_modules/\n")
        self.write("docs/node_modules/generated.md", "[ignored](absent.md)\n")
        failures = helper["check"](self.repo)
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("docs/content/guide.md: broken link", failures[0])

    def test_built_docs_preserve_routes_anchors_and_assets(self):
        check_site = runpy.run_path(str(SCRIPTS / "check-doc-site.py"))["check_site"]
        homepage = (
            '<link rel="shortcut icon" href="/favicon.svg">'
            '<link rel="canonical" href="https://docs.example.invalid/getting-started.html">'
            '<h1 id="getting-started">Getting started</h1><h2 id="setup">Setup</h2>'
            '<nav id="VPSidebarNav"><a href="/commands-and-usage.html#commands">Commands and usage</a></nav>'
            '<a href="https://example.invalid/external">External</a>'
            '<img src="/assets/example.svg">'
        )
        fixture = {
            "docs/public/CNAME": "docs.example.invalid\n",
            "docs/content/404.md": "---\ntitle: Page not found\ndescription: Missing page.\n---\n",
            "docs/content/guides/index.md": "---\ntitle: About this project\ndescription: Project introduction.\n---\n# About this project\n\n## Start here\n",
            "docs/content/guides/getting-started.md": "---\ntitle: Getting started\ndescription: Set up a machine.\n---\n# Getting started\n\n## Setup\n",
            "docs/content/guides/commands-and-usage.md": "---\ntitle: Commands and usage\ndescription: Everyday commands.\n---\n# Commands and usage\n\n## Commands\n",
            "docs/dist/index.html": (
                '<link rel="icon" href="/favicon.svg">'
                '<link rel="canonical" href="https://docs.example.invalid/">'
                '<header><a class="title" href="/">Dotfiles</a></header>'
                '<h1 id="about-this-project">About this project</h1><h2 id="start-here">Start here</h2>'
                '<nav id="VPSidebarNav">'
                '<a href="/getting-started.html">Getting started</a>'
                '<a href="/commands-and-usage.html">Commands and usage</a>'
                '</nav>'
            ),
            "docs/dist/getting-started.html": homepage,
            "docs/dist/commands-and-usage.html": (
                '<link rel="icon" href="/favicon.svg">'
                '<link rel="canonical" href="https://docs.example.invalid/commands-and-usage.html">'
                '<h1 id="commands-and-usage">Commands and usage</h1><h2 id="commands">Commands</h2>'
                '<a href="/getting-started.html#setup">Setup</a>'
            ),
            "docs/dist/404.html": (
                '<link rel="icon" href="/favicon.svg">'
                '<meta name="robots" content="noindex"><h1>Page not found</h1>'
            ),
            "docs/dist/favicon.svg": "<svg/>",
            "docs/dist/assets/example.svg": "<svg/>",
            "docs/dist/CNAME": "docs.example.invalid\n",
            "docs/dist/robots.txt": "User-agent: *\nAllow: /\nSitemap: https://docs.example.invalid/sitemap-index.xml\n",
            "docs/dist/sitemap-index.xml": (
                '<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
                '<sitemap><loc>https://docs.example.invalid/sitemap-0.xml</loc></sitemap></sitemapindex>'
            ),
            "docs/dist/sitemap-0.xml": (
                '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
                '<url><loc>https://docs.example.invalid/</loc></url>'
                '<url><loc>https://docs.example.invalid/getting-started.html</loc></url>'
                '<url><loc>https://docs.example.invalid/commands-and-usage.html</loc></url>'
                '</urlset>'
            ),
        }
        descriptions = {
            "index": "Project introduction.",
            "getting-started": "Set up a machine.",
            "commands-and-usage": "Everyday commands.",
        }
        for name, description in descriptions.items():
            fixture[f"docs/dist/{name}.html"] += f'<meta name="description" content="{description}">'
        homepage = fixture["docs/dist/getting-started.html"]
        for path, content in fixture.items():
            self.write(path, content)
        self.assertEqual(check_site(self.repo), [])
        cases = [
            ("docs/dist/index.html", None, "Missing built homepage"),
            ("docs/content/guides/index.md", None, "Missing Markdown homepage"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('id="about-this-project"', 'id="wrong"'), "missing heading anchors"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('<h1 id="about-this-project">About this project</h1>', '<h1 id="about-this-project">Welcome</h1>'), "Markdown title does not match page title"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('class="title"', 'class="other"'), "Homepage header must link"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('href="https://docs.example.invalid/"', 'href="https://docs.example.invalid/index.html"'), "expected canonical URL"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('<nav id="VPSidebarNav">', '<nav id="VPSidebarNav"><a href="/index.html">Dotfiles</a>'), "Homepage must not appear in sidebar navigation"),
            ("docs/dist/index.html", fixture["docs/dist/index.html"].replace('href="/getting-started.html"', 'href="/commands-and-usage.html"'), "missing from sidebar navigation"),
            ("docs/dist/commands-and-usage.html", None, "missing published route /commands-and-usage.html"),
            ("docs/dist/getting-started.html", homepage.replace('id="setup"', 'id="renamed"'), "missing heading anchors"),
            ("docs/dist/getting-started.html", homepage + "<h1>Extra title</h1>", "expected one page title"),
            ("docs/dist/getting-started.html", homepage + '<span id="setup"></span>', "duplicate IDs"),
            ("docs/dist/getting-started.html", homepage.replace("shortcut icon", "stylesheet"), "missing site favicon"),
            ("docs/dist/getting-started.html", homepage.replace("<h1", '<meta name="robots" content="index, noindex"><h1'), "noindex robots meta tag"),
            ("docs/dist/getting-started.html", homepage.replace("<h1", '<meta name="googlebot" content="none"><h1'), "noindex robots meta tag"),
            ("docs/dist/404.html", fixture["docs/dist/404.html"].replace('content="noindex"', 'content="index, follow"'), "missing noindex robots meta tag"),
            ("docs/dist/getting-started.html", homepage.replace("<h1", '<link rel="canonical" href="https://docs.example.invalid/getting-started"><h1'), "expected canonical URL"),
            ("docs/dist/getting-started.html", homepage.replace("#commands", "#absent"), "broken anchor"),
            ("docs/dist/getting-started.html", homepage.replace("commands-and-usage.html", "commands-and-usage.md"), "broken destination"),
            ("docs/dist/getting-started.html", homepage.replace(">Getting started<", ">Welcome<"), "filename and URL do not match title"),
            ("docs/content/guides/getting-started.md", "# Welcome\n\n## Setup\n", "Markdown title does not match page title"),
            ("docs/content/guides/getting-started.md", fixture["docs/content/guides/getting-started.md"].replace("title: Getting started", "title: Welcome"), "frontmatter title does not match"),
            ("docs/content/guides/getting-started.md", fixture["docs/content/guides/getting-started.md"].replace("description: Set up a machine.", "description: ''"), "nonempty single-line frontmatter description"),
            ("docs/dist/getting-started.html", homepage.replace("Set up a machine.", "Outdated description."), "rendered description does not match"),
            ("docs/dist/getting-started.html", homepage.replace(">Commands and usage<", ">Wrong label<"), "navigation label"),
            ("docs/dist/favicon.svg", None, "broken destination /favicon.svg"),
            ("docs/dist/assets/example.svg", None, "broken destination /assets/example.svg"),
            ("docs/dist/CNAME", "wrong.example.invalid\n", "published CNAME"),
            ("docs/dist/robots.txt", None, "Missing published robots.txt"),
            ("docs/dist/robots.txt", "User-agent: *\nAllow: /\n", "does not declare the canonical sitemap"),
            ("docs/dist/sitemap-index.xml", None, "Missing published sitemap-index.xml"),
            ("docs/dist/sitemap-0.xml", None, "Missing published sitemap"),
            ("docs/dist/sitemap-0.xml", fixture["docs/dist/sitemap-0.xml"].replace('invalid/</loc>', 'invalid/index.html</loc>'), "Sitemap URLs differ from canonical guide URLs"),
            ("docs/dist/sitemap-0.xml", fixture["docs/dist/sitemap-0.xml"].replace("getting-started.html", "getting-started"), "Sitemap URLs differ from canonical guide URLs"),
        ]
        for path, content, message in cases:
            with self.subTest(path=path, message=message):
                if content is None:
                    (self.repo / path).unlink()
                else:
                    self.write(path, content)
                self.assertIn(message, "\n".join(check_site(self.repo)))
                self.write(path, fixture[path])
        self.write("docs/content/reference/commands-and-usage.md",
                   fixture["docs/content/guides/commands-and-usage.md"])
        self.assertIn("source filenames must be unique", "\n".join(check_site(self.repo)))
        (self.repo / "docs/content/reference/commands-and-usage.md").unlink()
        self.write("docs/dist/guides/getting-started.html", homepage)
        self.assertIn("Published routes differ from flat source routes", "\n".join(check_site(self.repo)))

    def test_pending_site_publication_survives_a_successful_code_push(self):
        self.write("docs/content/example.md", "# Unpublished guide\n")
        previous = self.commit()
        self.env["BASE_SHA"] = previous
        self.write("cli/src/main.rs", "fn main() { println!(\"code-only push\"); }\n")
        self.env["HEAD_SHA"] = self.commit()
        self.env["DOCS_BASE_SHA"] = self.base
        self.assertTrue(self.script()["run_docs_site"])
        self.env["DOCS_BASE_SHA"] = previous
        self.assertFalse(self.script()["run_docs_site"])
        self.env.pop("DOCS_BASE_SHA")
        self.assertTrue(self.script()["run_docs_site"])
        self.env["DOCS_BASE_SHA"] = "invalid-baseline"
        self.script(success=False)

    def test_removed_and_renamed_site_inputs_select_a_build(self):
        self.write("docs/content/old.md", "# Guide\n")
        published = self.commit()
        self.env.update(BASE_SHA=published, DOCS_BASE_SHA=published)
        self.git("mv", "docs/content/old.md", "guide.md")
        self.env["HEAD_SHA"] = self.commit()
        self.assertTrue(self.script()["run_docs_site"])
        self.assertTrue(self.script()["run_docs_contracts"])

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
            '#!/bin/sh\nset -eu\n'
            'printf "called\\n" >> "$FIXTURE_GH_LOG"\n'
            '[ "$#" -eq 12 ] && [ "$1" = release ] && [ "$2" = list ] &&\n'
            '[ "$3" = --repo ] && [ "$4" = fixture/repository ] &&\n'
            '[ "$5" = --exclude-drafts ] && [ "$6" = --exclude-pre-releases ] &&\n'
            '[ "$7" = --limit ] && [ "$8" = 1 ] && [ "$9" = --json ] &&\n'
            '[ "${10}" = tagName ] && [ "${11}" = --jq ] &&\n'
            '[ "${12}" = ".[0].tagName // empty" ] || exit 90\n'
            'printf "%s\\n" "$FIXTURE_RELEASE_TAG"\nexit "$FIXTURE_GH_STATUS"\n',
            encoding="utf-8",
        )
        gh.chmod(0o755)
        self.env.update(
            PATH=str(mock) + os.pathsep + self.env["PATH"],
            FIXTURE_RELEASE_TAG=tag, FIXTURE_GH_STATUS=str(status),
            FIXTURE_GH_LOG=str(self.root / "gh-calls"),
        )

    def test_release_uses_net_changes_since_an_annotated_published_tag(self):
        self.git("tag", "-a", "-m", "published fixture", "v2026.01.01-2", self.base)
        self.mock_release(tag="v2026.01.01-2")
        self.write("cli/src/main.rs", "fn main() { println!(\"unpublished\"); }\n")
        self.commit()
        self.write("cli/src/main.rs", "fn main() {}\n")
        self.write("README.md", "binary changes were reverted before publishing\n")
        self.env["HEAD_SHA"] = self.commit()
        self.assertFalse(self.script("classify-release.sh")["run_release"])

    def test_failed_empty_release_lookup_is_not_an_initial_release(self):
        self.mock_release(tag="", status=1)
        self.script("classify-release.sh", success=False)
        self.assertEqual((self.root / "gh-calls").read_text(), "called\n")

    def test_invalid_release_head_is_rejected_before_api_lookup(self):
        self.mock_release(tag="")
        self.env["HEAD_SHA"] = "invalid-head"
        self.script("classify-release.sh", success=False)
        self.assertFalse((self.root / "gh-calls").exists())

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


class DocumentationPublicationTests(unittest.TestCase):
    def setUp(self):
        self.policy = runpy.run_path(str(SCRIPTS / "docs-publication-base.py"))

    def test_baseline_requires_a_successful_ancestor_deployment(self):
        sha = "a" * 40
        deployments = [{"id": 1, "sha": "b" * 40}, {"id": 2, "sha": sha}]
        for ancestry, expected in ((0, sha), (1, "")):
            with self.subTest(ancestry=ancestry), patch.dict(
                self.policy["publication_base"].__globals__, api=Mock(side_effect=[
                    deployments, [{"state": "failure"}],
                    [{"state": "inactive"}, {"state": "success"}],
                ]),
            ), patch.object(subprocess, "run", return_value=Mock(returncode=ancestry)):
                self.assertEqual(self.policy["publication_base"]("fixture/repo", "head"), expected)

    def test_missing_or_unavailable_baseline_requests_a_build(self):
        for response in ([], subprocess.CalledProcessError(1, "gh"), ValueError("bad json")):
            with self.subTest(response=response), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary) / "output"
                api = Mock(side_effect=response) if isinstance(response, Exception) else Mock(return_value=response)
                with patch.dict(self.policy["main"].__globals__, api=api), patch.dict(os.environ, {
                    "GITHUB_REPOSITORY": "fixture/repo", "GITHUB_SHA": "head",
                    "GITHUB_OUTPUT": str(output),
                }), contextlib.redirect_stdout(io.StringIO()):
                    self.policy["main"]()
                self.assertEqual(output.read_text(), "base_sha=\n")

    def test_pages_checks_the_exact_site_job_before_downloading(self):
        pages = CONTRACT["WORKFLOW"].with_name("pages.yml").read_text()
        script = pages.split("        run: |\n", 1)[1].split("      - name:", 1)[0]
        script = "\n".join(line[10:] for line in script.splitlines())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gh = root / "gh"
            gh.write_text(
                '#!/bin/sh\nset -eu\n'
                'case "$*" in\n'
                '  *actions/runs/123/jobs*) printf "%s\\n" "$SITE_RESULT"; exit "$API_STATUS" ;;\n'
                '  *git/ref/heads/main*) printf "%s\\n" "$CURRENT_SHA" ;;\n'
                '  *) exit 90 ;;\nesac\n'
            )
            gh.chmod(0o755)
            for result, current, api_status, expected, exit_code in (
                ("skipped", "head", "0", "false", 0),
                ("success", "head", "0", "true", 0),
                ("success", "newer", "0", "false", 0),
                ("failure", "head", "0", None, 1),
                ("cancelled", "head", "0", None, 1),
                ("", "head", "0", None, 1),
                ("success", "head", "1", None, 1),
            ):
                with self.subTest(result=result, current=current, api_status=api_status):
                    output = root / "output"
                    output.unlink(missing_ok=True)
                    completed = subprocess.run(["sh", "-eu", "-c", script], env=os.environ | {
                        "PATH": str(root) + os.pathsep + os.environ["PATH"],
                        "SITE_RESULT": result, "CURRENT_SHA": current, "API_STATUS": api_status,
                        "GITHUB_REPOSITORY": "fixture/repo", "CI_RUN_ID": "123",
                        "BUILD_SHA": "head", "GITHUB_OUTPUT": str(output),
                    }, capture_output=True, text=True)
                    self.assertEqual(completed.returncode, exit_code, completed.stderr)
                    self.assertEqual(output.read_text() if output.exists() else None,
                                     f"current={expected}\n" if expected else None)


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
