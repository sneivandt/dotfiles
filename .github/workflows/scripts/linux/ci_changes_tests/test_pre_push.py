"""Pre-push planning, coordinator, and local check regression tests."""

import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

from ci_changes_tests.helpers import LINUX, PRE_PUSH, SCRIPTS, WINDOWS, classify


class PrePushPlanTests(unittest.TestCase):
    def test_native_docs_reuse_keeps_all_implementation_checks(self):
        selection = classify(["cli/Cargo.toml"])
        commands, deferred = PRE_PUSH["plan"](selection, "1.91")
        reused, remaining = PRE_PUSH["plan"](selection, "1.91", docs_checked=True)
        expected = [
            (label, directory, [argument for argument in command if argument != "docs"])
            for label, directory, command in commands
            if label != "documentation dependencies"
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
        self.assertNotIn("mutation", deferred)
        self.assertIn("roundtrip_windows", deferred)
        self.assertFalse(
            any("update" in command or "remove" in command for _, _, command in commands)
        )

    def test_config_and_desktop_select_their_regressions(self):
        commands, _ = PRE_PUSH["plan"](classify(["symlinks/config/quickshell/bar/Bar.qml"]), "1.91")
        flattened = [argument for _, _, command in commands for argument in command]
        for argument in ("config_drift", "qml", "tests/desktop/quickshell/python"):
            self.assertIn(argument, flattened)
        self.assertNotIn("clippy", flattened)

    def test_application_fixtures_follow_selected_applications(self):
        for path, expected, absent in (
            ("symlinks/zshrc", "tests/shell/test-shell-config.py", "tests/vim"),
            ("symlinks/vim/vimrc", "tests/vim", "tests/shell/test-shell-config.py"),
        ):
            commands, _ = PRE_PUSH["plan"](classify([path]), "1.91")
            flattened = " ".join(arg for _, _, command in commands for arg in command)
            self.assertIn(expected, flattened)
            self.assertNotIn(absent, flattened)

    def test_local_plan_tracks_fixture_inputs_without_running_installations(self):
        desktop = "tests/desktop/hypr"
        maintenance = "tests/hooks/test-maintenance.py"
        shell = "tests/shell/test-shell-config.py"
        for path, fixtures in (
            ("conf/systemd-units.toml", {desktop}),
            ("symlinks/config/systemd/user/dotfiles-first-login.service", {desktop}),
            ("symlinks/config/systemd/user/xdg-user-directories.service", {desktop, shell}),
            ("symlinks/config/user-dirs.dirs", {shell}),
            ("symlinks/config/git/aliases", {maintenance, shell}),
            ("symlinks/config/git/config", set()),
        ):
            with self.subTest(path=path):
                commands, _ = PRE_PUSH["plan"](classify([path]), "1.91")
                args = {arg for _, _, command in commands for arg in command}
                self.assertEqual(args & {desktop, maintenance, shell}, fixtures)
        _, deferred = PRE_PUSH["plan"](classify(["dotfiles.ps1"]), "1.91")
        self.assertIn("roundtrip_windows", deferred)

    def test_checker_changes_request_full_ci_without_binary_release(self):
        for path in (
            LINUX + "pre-push.py",
            LINUX + "check.py",
            LINUX + "check.sh",
            WINDOWS + "Check.ps1",
        ):
            with self.subTest(path=path):
                selected = classify([path])
                self.assertTrue(selected["run_rust_checks"])
                self.assertTrue(selected["run_hook_inputs"])
                self.assertTrue(selected["run_desktop_qml"])
                self.assertFalse(selected["run_release"])

    @unittest.skipUnless(sys.platform == "linux", "requires POSIX fixture executables")
    def test_missing_tools_fail_strict_checker(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripts = root / ".github/workflows/scripts/linux"
            scripts.mkdir(parents=True)
            (root / "cli").mkdir()
            shutil.copyfile(SCRIPTS / "check.py", scripts / "check.py")
            shutil.copyfile(SCRIPTS / "check.sh", scripts / "check.sh")
            (root / "dirname").symlink_to(shutil.which("dirname"))
            (root / "python3").symlink_to(sys.executable)
            for cmd in (
                [sys.executable, "-B", str(scripts / "check.py")],
                ["/bin/sh", str(scripts / "check.sh")],
            ):
                for strict, expected in ((False, 0), (True, 1)):
                    with self.subTest(cmd=cmd[-1], strict=strict):
                        result = subprocess.run(
                            [*cmd, *(["--strict"] if strict else []), "shell"],
                            env={"PATH": str(root)},
                            text=True,
                            capture_output=True,
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
        self.assertEqual(
            stages[stages.index("--strict") + 1 :], ["clippy", "test", "config", "powershell"]
        )
        flattened = [arg for _, _, command in commands for arg in command]
        for unnecessary in ("fmt", "rustdoc", "audit", "deny", "x86_64-pc-windows-gnu"):
            self.assertNotIn(unnecessary, flattened)
        wrapper = next(
            command for label, _, command in commands if label == "isolated PowerShell wrapper"
        )
        self.assertIn("if (-not (Test-IsolatedWrapperPath)) { exit 1 }", wrapper[-1])
        self.assertIn("if (-not (Test-CargoArtifactPath)) { exit 1 }", wrapper[-1])

    def test_windows_plan_runs_prompt_without_linux_desktop_script_suites(self):
        desktop_commands = PRE_PUSH["windows_plan"](
            classify(
                [
                    "symlinks/config/hypr/scripts/lock-screen.sh",
                    "symlinks/config/quickshell/helpers/network_helper.py",
                ]
            ),
            docs_checked=True,
        )
        flattened = [arg for _, _, command in desktop_commands for arg in command]
        self.assertNotIn("tests/desktop/hypr", flattened)
        self.assertNotIn("tests/desktop/quickshell/python", flattened)

        prompt_commands = PRE_PUSH["windows_plan"](
            classify(["symlinks/config/powershell/Microsoft.PowerShell_profile.ps1"]),
            docs_checked=True,
        )
        prompt_args = [arg for _, _, command in prompt_commands for arg in command]
        self.assertIn("tests/shell/Test-Prompt.ps1", prompt_args)

    def test_windows_receipt_omits_only_redundant_compilation_and_analyzer(self):
        selection = classify(["cli/src/main.rs", "dotfiles.ps1"])
        original, deferred = PRE_PUSH["plan"](selection, "1.91", docs_checked=True)
        reused, remaining = PRE_PUSH["plan"](
            selection, "1.91", docs_checked=True, windows_checked=True
        )
        expected = [
            (label, directory, [arg for arg in command if arg != "powershell"])
            if label == "core checks"
            else (label, directory, command)
            for label, directory, command in original
            if label != "Windows compilation (no runtime coverage)"
        ]
        self.assertEqual(reused, expected)
        self.assertEqual(remaining, deferred)
        self.assertTrue(any("test" in command for _, _, command in reused))

    def test_wsl_worker_uses_exact_range_and_receipts(self):
        for unavailable in (False, True):
            with (
                self.subTest(unavailable=unavailable),
                patch("shutil.which", return_value="wsl"),
                patch(
                    "subprocess.check_output",
                    return_value="/mnt/c/repo with spaces/pre-push.py\n",
                ) as output,
            ):
                command = PRE_PUSH["wsl_command"](
                    "base-sha", "head-sha", True, npm_tls_unavailable=unavailable
                )
            self.assertEqual(
                command[:5],
                ["wsl", "--exec", "python3", "-B", "/mnt/c/repo with spaces/pre-push.py"],
            )
            receipt = "--npm-tls-unavailable" if unavailable else "--docs-checked"
            self.assertEqual(
                command[5:],
                [
                    "--base",
                    "base-sha",
                    "--head",
                    "head-sha",
                    receipt,
                    "head-sha",
                    "--windows-checked",
                    "head-sha",
                    "--full",
                ],
            )
            self.assertEqual(output.call_args.args[0][:4], ["wsl", "--exec", "wslpath", "-u"])
        with (
            patch("shutil.which", return_value=None),
            self.assertRaisesRegex(RuntimeError, "WSL is unavailable"),
        ):
            PRE_PUSH["wsl_command"]("base", "head", False)

    def test_validation_cache_is_separate_by_checkout_and_platform(self):
        root = Path(tempfile.gettempdir()) / "source checkout"
        cache = PRE_PUSH["target_cache"](root)
        self.assertTrue(
            cache.is_relative_to(Path.home() / ".cache/dotfiles-pre-push" / sys.platform)
        )
        self.assertNotEqual(cache, root / "cli/target")
        self.assertEqual(cache, PRE_PUSH["target_cache"](root))
        self.assertNotEqual(cache, PRE_PUSH["target_cache"](root.with_name("another checkout")))
        other_platform = "linux" if sys.platform == "win32" else "win32"
        with patch.object(sys, "platform", other_platform):
            self.assertNotEqual(cache, PRE_PUSH["target_cache"](root))

    def test_validation_cache_only_clears_its_own_artifacts_under_pressure(self):
        minimum = PRE_PUSH["MIN_CACHE_FREE"]
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            cache = Path(directory) / "target"
            cache.mkdir()
            (cache / "artifact").touch()
            sibling = cache.with_name("other-build")
            sibling.touch()
            functions = PRE_PUSH["locked_cache"].__wrapped__.__globals__
            with (
                patch.dict(functions, {"free_cache_space": Mock(return_value=minimum)}),
                PRE_PUSH["locked_cache"](cache),
            ):
                self.assertTrue((cache / "artifact").is_file())
                with (
                    self.assertRaisesRegex(RuntimeError, "Could not lock validation cache"),
                    PRE_PUSH["locked_cache"](cache),
                ):
                    self.fail("A second validation obtained the lock")
            with (
                patch.dict(
                    functions, {"free_cache_space": Mock(side_effect=[minimum - 1, minimum])}
                ),
                PRE_PUSH["locked_cache"](cache),
            ):
                self.assertFalse(cache.exists())
                self.assertTrue(sibling.is_file())
            with (
                patch.dict(functions, {"free_cache_space": Mock(return_value=minimum - 1)}),
                self.assertRaisesRegex(RuntimeError, "Free at least 10 GiB"),
                PRE_PUSH["locked_cache"](cache),
            ):
                self.fail("Validation started with insufficient free space")

    def test_cache_preview_does_not_create_or_remove_anything(self):
        with (
            tempfile.TemporaryDirectory() as directory,
            contextlib.redirect_stdout(io.StringIO()) as output,
        ):
            cache = Path(directory) / "uncreated" / "target"
            PRE_PUSH["cache_status"](cache)
            self.assertIn(str(cache), output.getvalue())
            self.assertFalse(cache.parent.exists())

    def test_explicit_baseline_does_not_query_github(self):
        with (
            patch.dict(
                PRE_PUSH["comparison_base"].__globals__, {"git": lambda *args: "original-sha"}
            ),
            patch("subprocess.check_output") as output,
        ):
            self.assertEqual(
                PRE_PUSH["comparison_base"](".", "original", "head"), ("original-sha", False)
            )
            output.assert_not_called()

    def test_default_baseline_recovers_pending_source_changes(self):
        for remote in (
            "https://github.com/fixture/repository.git",
            "git@github.com:fixture/repository.git",
        ):
            for conclusion in ("success", "failure", "cancelled", None):
                with self.subTest(remote=remote, conclusion=conclusion):
                    outputs = [json.dumps({"workflow_runs": [{"conclusion": conclusion}]})]
                    if conclusion != "success":
                        outputs.append(
                            json.dumps({"workflow_runs": [{"head_sha": "original-sha"}]})
                        )
                    with (
                        patch.dict(
                            PRE_PUSH["comparison_base"].__globals__,
                            {
                                "git": lambda root, *args, remote=remote: (
                                    remote if args[0] == "remote" else "tip-sha"
                                ),
                            },
                        ),
                        patch("subprocess.check_output", side_effect=outputs) as output,
                        patch("subprocess.run") as run,
                    ):
                        run.return_value.returncode = 0
                        expected = (
                            ("tip-sha", False)
                            if conclusion == "success"
                            else ("original-sha", True)
                        )
                        self.assertEqual(PRE_PUSH["comparison_base"](".", None, "head"), expected)
                        self.assertIn(
                            "repos/fixture/repository/actions/workflows/ci.yml/runs",
                            output.call_args.args[0],
                        )

    def test_missing_or_unverifiable_baseline_fails_closed(self):
        with patch.dict(
            PRE_PUSH["comparison_base"].__globals__,
            {
                "git": lambda root, *args: (
                    "https://github.com/fixture/repository.git" if args[0] == "remote" else "tip"
                ),
            },
        ):
            with (
                patch("subprocess.check_output", return_value='{"workflow_runs": []}'),
                self.assertRaisesRegex(RuntimeError, "No successful CI ancestor"),
            ):
                PRE_PUSH["comparison_base"](".", None, "head")
            with (
                patch(
                    "subprocess.check_output", side_effect=subprocess.CalledProcessError(1, ["gh"])
                ),
                self.assertRaises(subprocess.CalledProcessError),
            ):
                PRE_PUSH["comparison_base"](".", None, "head")

    def test_windows_runs_native_before_linux_and_never_continues_after_failure(self):
        worker = ["fixture-wsl", "--head", "head-sha"]
        for failure in (False, True):
            events = []

            def execute(*args, events=events, failure=failure):
                events.append("native")
                if failure:
                    raise subprocess.CalledProcessError(17, ["fixture-native"])

            def run(command, events=events, **kwargs):
                if command == worker:
                    events.append("linux")

            replacements = {
                "git": lambda root, *args: (
                    str(SCRIPTS.parents[3]) if "--show-toplevel" in args else "head-sha"
                ),
                "comparison_base": lambda *args: ("base-sha", True),
                "committed_policy": lambda *args: {
                    "changed_paths": lambda *args: ["cli/src/main.rs"],
                    "classify": classify,
                },
                "execute": execute,
                "wsl_command": Mock(return_value=worker),
            }
            # main reads only the committed Cargo rust-version after selection.
            real_git = replacements["git"]
            replacements["git"] = lambda root, *args, real_git=real_git: (
                '[package]\nrust-version = "1.91"\n' if args[0] == "show" else real_git(root, *args)
            )
            with (
                patch.dict(PRE_PUSH["main"].__globals__, replacements),
                patch.object(sys, "platform", "win32"),
                patch.object(
                    sys,
                    "argv",
                    ["pre-push.py"],
                ),
                patch("subprocess.run", side_effect=run),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                if failure:
                    with self.assertRaises(subprocess.CalledProcessError):
                        PRE_PUSH["main"]()
                else:
                    PRE_PUSH["main"]()
            self.assertEqual(events, ["native"] if failure else ["native", "linux"])
            replacements["wsl_command"].assert_called_once_with(
                "base-sha", "head-sha", True, npm_tls_unavailable=False
            )

    def test_tls_receipt_defers_only_docs_and_never_hides_other_failures(self):
        for platform in ("win32", "linux"):
            for failure in (False, True):
                with self.subTest(platform=platform, failure=failure):
                    execute = Mock(
                        side_effect=subprocess.CalledProcessError(17, ["fixture-check"])
                        if failure
                        else None
                    )
                    worker = ["fixture-wsl"]
                    replacements = {
                        "git": lambda root, *args: (
                            '[package]\nrust-version = "1.91"\n'
                            if args[0] == "show"
                            else "head-sha"
                        ),
                        "comparison_base": lambda *args: ("base-sha", False),
                        "committed_policy": lambda *args: {
                            "changed_paths": lambda *args: ["conf/symlinks.toml"],
                            "classify": classify,
                        },
                        "execute": execute,
                        "wsl_command": Mock(return_value=worker),
                    }
                    output = io.StringIO()
                    with (
                        patch.dict(PRE_PUSH["main"].__globals__, replacements),
                        patch.object(sys, "platform", platform),
                        patch.object(
                            sys,
                            "argv",
                            ["pre-push.py", "--npm-tls-unavailable", "head-sha"],
                        ),
                        patch("subprocess.run") as run,
                        contextlib.redirect_stdout(output),
                    ):
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
                            "base-sha",
                            "head-sha",
                            False,
                            npm_tls_unavailable=True,
                        )

    def test_success_and_tls_unavailability_receipts_are_mutually_exclusive(self):
        with (
            patch.object(
                sys,
                "argv",
                [
                    "pre-push.py",
                    "--docs-checked",
                    "head",
                    "--npm-tls-unavailable",
                    "head",
                ],
            ),
            contextlib.redirect_stderr(io.StringIO()),
            self.assertRaises(SystemExit) as error,
        ):
            PRE_PUSH["main"]()
        self.assertEqual(error.exception.code, 2)

    def test_mismatched_receipt_fails_before_executing_checks(self):
        replacements = {
            "git": lambda root, *args: "other-sha" if args[-1] == "old^{commit}" else "head-sha",
            "comparison_base": lambda *args: ("base-sha", False),
            "execute": Mock(),
        }
        for option in ("--docs-checked", "--windows-checked", "--npm-tls-unavailable"):
            with (
                patch.dict(PRE_PUSH["main"].__globals__, replacements),
                patch.object(sys, "platform", "linux"),
                patch.object(sys, "argv", ["pre-push.py", option, "old"]),
                self.assertRaisesRegex(RuntimeError, "exact commit"),
            ):
                PRE_PUSH["main"]()
        replacements["execute"].assert_not_called()


class LocalCheckTests(unittest.TestCase):
    def test_tool_failures_are_not_optional_tool_skips(self):
        stages = (
            "fmt",
            "clippy",
            "test",
            "rustdoc",
            "config",
            "docs",
            "ci",
            "shell",
            "powershell",
            "python",
            "lua",
            "audit",
            "deny",
            "msrv",
            "qml",
            "hooks",
            "wrapper",
            "desktop",
        )
        with tempfile.TemporaryDirectory(prefix="dotfiles-check-status-") as temporary:
            root = Path(temporary)
            scripts = root / LINUX
            scripts.mkdir(parents=True)
            for rel in (
                "tests/desktop/quickshell/test-desktop-qml.sh",
                "tests/hooks/test-hook-inputs.py",
                "tests/hooks/test-maintenance.py",
                "tests/hooks/test-git-hooks.py",
                "tests/wrapper/test-parity.py",
                "tests/wrapper/test-shell-wrapper.sh",
                LINUX + "check-ci-contract.py",
                LINUX + "test-ci-changes.py",
            ):
                target = root / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                if rel.endswith(".sh"):
                    target.write_text(
                        '#!/bin/sh\nexit "$CHECK_STATUS"\n', encoding="utf-8", newline="\n"
                    )
                else:
                    target.write_text(
                        'import os, sys\nsys.exit(int(os.environ["CHECK_STATUS"]))\n',
                        encoding="utf-8",
                        newline="\n",
                    )
            for rel in (
                "tests/desktop/hypr/test_fixture.py",
                "tests/desktop/quickshell/python/test_fixture.py",
            ):
                target = root / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(
                    "import os, unittest\n"
                    "class FixtureTest(unittest.TestCase):\n"
                    "    def test_status(self):\n"
                    "        self.assertEqual(os.environ['CHECK_STATUS'], '0')\n",
                    encoding="utf-8",
                    newline="\n",
                )
            (root / "cli").mkdir()
            (root / "cli/Cargo.toml").write_text(
                '[package]\nrust-version = "1.91"\n',
                encoding="utf-8",
                newline="\n",
            )
            (root / "cli/rust-toolchain.toml").write_text(
                '[toolchain]\nchannel = "1.95.0"\n',
                encoding="utf-8",
                newline="\n",
            )
            for script_name in ("check.py", "check.sh"):
                shutil.copyfile(SCRIPTS / script_name, scripts / script_name)
            for name in ("test-docs.sh", "test-static-analysis.sh", "test-workflows.sh"):
                (scripts / name).write_text(
                    '#!/bin/sh\nexit "$CHECK_STATUS"\n',
                    encoding="utf-8",
                    newline="\n",
                )
            tools = root / "tools"
            tools.mkdir()
            rustc = tools / "rustc"
            rustc.write_text(
                '#!/bin/sh\necho "rustc 1.95.0 (fixture)"\n',
                encoding="utf-8",
                newline="\n",
            )
            rustc.chmod(0o755)
            for name in (
                "cargo",
                "rustup",
                "shellcheck",
                "pwsh",
                "ruff",
                "luacheck",
                "actionlint",
                "qmltestrunner",
            ):
                tool = tools / name
                tool.write_text(
                    "#!/bin/sh\n"
                    '[ "${2:-}" = --version ] && exit 0\n'
                    '[ "${1:-}" = toolchain ] && exit 0\n'
                    'exit "$CHECK_STATUS"\n',
                    encoding="utf-8",
                    newline="\n",
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
                        capture_output=True,
                        text=True,
                        timeout=120,
                    )
                    self.assertEqual(
                        result.returncode, int(status != 0), result.stdout + result.stderr
                    )
                    self.assertNotIn("SKIP", result.stdout)
                    for stage in stages:
                        self.assertRegex(
                            result.stdout,
                            rf"(?m)^\s+{'PASS' if status == 0 else 'FAIL'} {stage}$",
                        )

    def test_command_start_failures_do_not_pass(self):
        import importlib.util

        spec = importlib.util.spec_from_file_location("check_runner", SCRIPTS / "check.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        runner = module.Runner(SCRIPTS.parents[3], module.Console())
        with contextlib.redirect_stdout(io.StringIO()):
            status = runner.run_cmd(["dotfiles-fixture-command-that-does-not-exist", "--version"])
        self.assertEqual(status, "FAIL")

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell is unavailable")
    def test_windows_skips_are_reported_and_strict_mode_fails(self):
        with tempfile.TemporaryDirectory(prefix="dotfiles-check-windows-") as temporary:
            root = Path(temporary)
            script = root / WINDOWS / "Check.ps1"
            script.parent.mkdir(parents=True)
            (root / LINUX).mkdir(parents=True)
            (root / "cli").mkdir()
            shutil.copyfile(SCRIPTS.parent / "windows/Check.ps1", script)
            shutil.copyfile(SCRIPTS / "check.py", root / LINUX / "check.py")
            py_dir = root / "pybin"
            py_dir.mkdir()
            (py_dir / "python3").symlink_to(sys.executable)
            env = os.environ | {"PATH": str(py_dir), "DOTFILES_CHECK_STRICT": "0"}
            for strict, inherited in ((False, False), (True, False), (False, True)):
                with self.subTest(strict=strict, inherited=inherited):
                    result = subprocess.run(
                        [
                            shutil.which("pwsh"),
                            "-NoProfile",
                            "-NonInteractive",
                            "-File",
                            str(script),
                            *(["-Strict"] if strict else []),
                            "fmt",
                        ],
                        env=env | {"DOTFILES_CHECK_STRICT": "1" if inherited else "0"},
                        capture_output=True,
                        text=True,
                        timeout=120,
                    )
                    self.assertEqual(
                        result.returncode,
                        int(strict or inherited),
                        result.stdout + result.stderr,
                    )
                    self.assertIn("SKIP fmt", result.stdout)
                    self.assertNotIn("All requested checks passed", result.stdout)
                    if not (strict or inherited):
                        self.assertIn(
                            "Checks completed with skips; coverage is incomplete.", result.stdout
                        )

    def test_windows_stage_selection_and_native_stages(self):
        import importlib.util

        spec = importlib.util.spec_from_file_location("check_runner_win", SCRIPTS / "check.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with patch.object(sys, "platform", "win32"):
            default_stages, optional_stages = module.platform_stages()
            self.assertIn("docs", default_stages)
            self.assertIn("ci", default_stages)
            self.assertIn("python", default_stages)
            self.assertNotIn("shell", default_stages)
            self.assertIn("shell", optional_stages)

            runner = module.Runner(SCRIPTS.parents[3], module.Console())
            with (
                patch.object(runner, "have", return_value=False),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(runner.stage_docs(), "skip")
                self.assertEqual(runner.stage_python(), "skip")

            captured = []

            def fake_run_cmd(argv, *, cwd=None, env=None):
                captured.append((argv, cwd))
                return "pass"

            with (
                patch.object(runner, "have", return_value=True),
                patch.object(
                    runner,
                    "run_cmd",
                    side_effect=fake_run_cmd,
                ),
            ):
                self.assertEqual(runner.stage_python(), "pass")
            self.assertEqual(len(captured), 2)
            check_argv, check_cwd = captured[0]
            fmt_argv, fmt_cwd = captured[1]
            config = str(SCRIPTS.parents[3] / ".github/linters/ruff.toml")
            self.assertEqual(check_argv[:4], ["ruff", "check", "--config", config])
            self.assertEqual(check_argv[4], "--")
            self.assertIn(str(SCRIPTS.parents[3]), check_argv[5:])
            self.assertIn(
                str(SCRIPTS.parents[3] / "symlinks/local/bin/code-insiders"), check_argv[5:]
            )
            self.assertEqual(check_cwd, SCRIPTS.parents[3])
            self.assertEqual(fmt_argv[:5], ["ruff", "format", "--check", "--config", config])
            self.assertEqual(fmt_argv[5], "--")
            self.assertEqual(fmt_argv[6:], check_argv[5:])
            self.assertEqual(fmt_cwd, SCRIPTS.parents[3])
