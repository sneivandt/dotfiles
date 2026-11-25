"""Change-classification regression tests."""

import unittest

from ci_changes_tests.helpers import LINUX, POLICY, WINDOWS, classify


class ClassificationTests(unittest.TestCase):
    def test_exact_job_selection(self):
        config = {"validate_config", "config_drift", "build_linux"}
        profiles = config | {
            "profile_linux", "profile_windows", "roundtrip_linux", "roundtrip_windows",
            "build_windows",
        }
        rust = {"rust_checks", "rust_fmt", "build_linux", "build_windows"}
        desktop_scripts = {"managed_scripts", "lock"}
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
                "release",
            }),
            ("cli/src/domains/ai/apm/scripts/workflow_autopilot.py", rust | profiles | {
                "app_tests", "app_windows", "wrapper_linux", "wrapper_windows", "release", "apm",
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
            ("packages/apm/PKGBUILD", config | {"rust_checks", "lint", "apm"}),
            ("packages/apm/release.json", config | {"rust_checks", "lint", "apm"}),
            ("packages/example/example.install", config | {"rust_checks", "lint"}),
            ("symlinks/apm/config/base.yml", config | {"apm"}),
            ("symlinks/apm/plugins/dot-skill/.apm/skills/example/SKILL.md", config | {"apm"}),
            (LINUX + "test-apm.py", {"apm", "validate_config", "build_linux"}),
            (".gitignore", set()),
            ("conf/agent-settings.toml", profiles),
            ("conf/chmod.toml", profiles | {"rust_checks"}),
            ("conf/system-files.toml", profiles | {"rust_checks"}),
            ("conf/systemd-units.toml", profiles | desktop_scripts),
            ("system/pam.d/login", config | {"profile_linux", "roundtrip_linux", "rust_checks"}),
            ("symlinks/config/git/config", config | {"app_tests", "app_windows", "build_windows"}),
            ("symlinks/config/git/aliases", config | {"git_hooks", "app_tests", "app_windows", "build_windows"}),
            ("symlinks/config/git/ignore", config | {"app_tests", "app_windows", "build_windows"}),
            ("symlinks/config/git/windows", config | {"app_tests", "app_windows", "build_windows"}),
            ("symlinks/config/git/windows-desktop", config | {"app_tests", "app_windows", "build_windows"}),
            ("symlinks/config/tmux/tmux.conf", config | {"app_tests"}),
            ("symlinks/config/readline/inputrc", config | {"app_tests"}),
            ("symlinks/config/pacman.conf", config | {"app_tests"}),
            ("symlinks/vim/init.vim", config | {"app_tests"}),
            ("symlinks/config/zsh/prompt.zsh", config | {"app_tests"}),
            ("symlinks/config/bash/prompt.bash", config | {"app_tests", "lint"}),
            ("symlinks/bash_profile", config | {"app_tests", "lint"}),
            ("symlinks/bashrc", config | {"app_tests", "lint"}),
            ("symlinks/local/bin/code-insiders", config | desktop_scripts | {"rust_checks", "app_tests"}),
            ("symlinks/config/powershell/Module.psd1",
             config | {"lint", "managed_scripts", "prompt"}),
            ("symlinks/config/wgetrc", config | {"app_tests"}),
            ("symlinks/config/user-dirs.dirs", config | {"app_tests"}),
            ("symlinks/config/hypr/conf/appearance.lua",
             config | {"managed_scripts", "lock", "desktop_qml"}),
            ("symlinks/config/hypr/conf/resize-on-border.lua",
             config | {"managed_scripts", "lock", "desktop_qml"}),
            ("symlinks/config/hypr/hyprland.lua", config | {"desktop_qml"}),
            ("symlinks/config/hypr/hyprlock.conf", config | {"desktop_qml"}),
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
            ("dotfiles.ps1", {"lint", "wrapper_windows", "roundtrip_windows", "build_windows"}),
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
            (WINDOWS + "shell_wrapper_tests/Test-Bootstrap.ps1", {"wrapper_windows", "build_windows", "lint"}),
            (WINDOWS + "Test-InstallUninstall.ps1", {"roundtrip_windows", "build_windows", "lint"}),
            (WINDOWS + "Test-Applications.ps1", {"app_windows", "build_windows", "lint"}),
            ("symlinks/config/git/templates/hooks/post-rewrite",
             config | {"app_tests", "app_windows", "build_windows", "git_hooks", "lint", "rust_checks"}),
            ("symlinks/config/git/templates/hooks.local/pre-push",
             config | {"app_tests", "app_windows", "build_windows", "git_hooks", "lint", "rust_checks"}),
            ("symlinks/config/systemd/user/clean-home-tmp.service", config | desktop_scripts | {"git_hooks", "desktop_qml"}),
            ("symlinks/config/systemd/user/clean-screenshots.service", config | desktop_scripts | {"git_hooks", "desktop_qml"}),
            ("symlinks/config/systemd/user/dotfiles-first-login.service",
             config | desktop_scripts | {"desktop_qml", "rust_checks", "roundtrip_linux"}),
            ("symlinks/config/systemd/user/xdg-user-directories.service",
             config | desktop_scripts | {"desktop_qml", "app_tests"}),
            ("symlinks/config/powershell/Microsoft.PowerShell_profile.ps1",
             config | {"prompt", "roundtrip_windows", "managed_scripts", "build_windows", "lint"}),
            ("symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
             config | {"roundtrip_windows", "build_windows"}),
            ("symlinks/config/systemd/user/quickshell.service", config | desktop_scripts | {"desktop_qml"}),
            ("symlinks/config/systemd/user/clean-home-tmp.timer", config | {"desktop_qml"}),
            (LINUX + "test-maintenance.py", {"git_hooks", "hook_inputs"}),
            (LINUX + "test-nvim-lock.lua", {"app_tests", "build_linux"}),
            (LINUX + "test-nvim-config.lua", {"app_tests", "build_linux"}),
            (LINUX + "test-shell-config.py", {"app_tests", "build_linux"}),
            (LINUX + "shell_config_tests/test_profile_and_env.py", {"app_tests", "build_linux"}),
            (LINUX + "test-hook-inputs.sh", {"hook_inputs", "build_linux", "lint"}),
            (LINUX + "check-ci-contract.py", set()),
            (LINUX + "test-ci-changes.py", set()),
            (LINUX + "ci_changes_tests/test_classification.py", set()),
            (LINUX + "select-mutations.py", set()),
            (LINUX + "run-mutations.py", set()),
            (LINUX + "report-mutations.py", set()),
            (".github/workflows/mutation.yml", set()),
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
                    if name != "run_release"
                ))
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
            (["symlinks/config/git/aliases"], [], ["git", "zsh"]),
            (["symlinks/config/git/windows"], [], ["git", "zsh"]),
            (["symlinks/config/user-dirs.dirs"], [], ["zsh"]),
            (["symlinks/config/systemd/user/xdg-user-directories.service"], [], ["zsh"]),
            (["symlinks/vim/init.vim"], [], ["vim", "nvim"]),
            ([LINUX + "test-nvim-config.lua"], [], ["nvim"]),
            ([LINUX + "test-shell-config.py"], [], ["zsh"]),
            (["symlinks/bashrc", "symlinks/config/wgetrc"], ["ShellCheck"], ["zsh"]),
            (["symlinks/config/bash/prompt.bash"], ["ShellCheck"], ["zsh"]),
            (["packages/example/example.install"], ["ShellCheck"], []),
            (["symlinks/config/powershell/Module.psd1"], ["PSScriptAnalyzer"], []),
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

    def test_uwsm_session_script_selects_shell_config_job(self):
        outputs = classify(["symlinks/config/uwsm/env-hyprland.d/session.sh"])
        self.assertTrue(outputs["run_app_tests"])
        self.assertEqual(
            [item["application"] for item in outputs["app_matrix"]["include"]],
            ["zsh"],
        )
        self.assertTrue(outputs["run_lint"])
        self.assertEqual(
            [item["name"] for item in outputs["lint_matrix"]["include"]],
            ["ShellCheck"],
        )
        self.assertTrue(outputs["run_build_linux"])
        self.assertTrue(outputs["run_validate_config"])
        self.assertTrue(outputs["run_config_drift"])

    def test_code_insiders_wrapper_selects_shell_launcher_and_rust_checks(self):
        outputs = classify(["symlinks/local/bin/code-insiders"])
        self.assertTrue(outputs["run_app_tests"])
        self.assertEqual(
            [item["application"] for item in outputs["app_matrix"]["include"]],
            ["zsh"],
        )
        self.assertTrue(outputs["run_managed_scripts"])
        self.assertTrue(outputs["run_lock"])
        self.assertTrue(outputs["run_rust_checks"])
        self.assertTrue(outputs["run_build_linux"])
        self.assertTrue(outputs["run_validate_config"])
        self.assertTrue(outputs["run_config_drift"])

    def test_binary_inputs(self):
        for path in ("cli/src/main.rs", "cli/src/data.json", "cli/src/embedded.md", "cli/build.rs",
                     "cli/.cargo/config.toml", "cli/rust-toolchain.toml", "cli/Cargo.toml",
                     ".cargo/config.toml", "rust-toolchain.toml"):
            with self.subTest(path=path):
                self.assertTrue(classify([path])["run_release"])
        for path in ("cli/tests/integration/update_command.rs", "cli/benches/fixture.json",
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
