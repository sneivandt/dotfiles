"""Change-classification regression tests."""

import unittest

from ci_changes_tests.helpers import LINUX, POLICY, classify


class ClassificationTests(unittest.TestCase):
    def test_exact_job_selection(self):
        config = {"validate_config", "config_drift", "build_linux"}
        profiles = config | {
            "profile_linux", "profile_windows", "roundtrip_linux", "roundtrip_windows",
            "build_windows",
        }
        rust = {"rust_checks", "rust_fmt", "build_linux", "build_windows"}
        desktop_scripts = {"desktop_scripts", "lock"}
        cases = [
            ("README.md", set()),
            ("tests/desktop/quickshell/test-desktop-session.py", {"desktop_qml", "lint"}),
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
                "app_tests", "app_windows", "wrapper_linux", "wrapper_windows",
                "release", "apm", "lint",
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
            (".github/linters/ruff.toml", {"lint"}),
            (".github/linters/luacheckrc", {"lint"}),
            (LINUX + "install-ruff.sh", {"lint"}),
            (LINUX + "install-luacheck.sh", {"lint"}),
            ("conf/packages.toml", profiles | {"app_tests", "app_windows"}),
            ("packages/apm/PKGBUILD", config | {"rust_checks", "lint", "apm"}),
            ("packages/apm/release.json", config | {"rust_checks", "lint", "apm"}),
            ("packages/example/example.install", config | {"rust_checks", "lint"}),
            ("symlinks/apm/config/base.yml", config | {"apm"}),
            ("symlinks/apm/plugins/dot-skill/.apm/skills/example/SKILL.md", config | {"apm"}),
            ("tests/integration/test-apm.py", {"apm", "validate_config", "build_linux", "lint"}),
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
            ("symlinks/vim/init.vim", config | {"app_tests"}),
            ("symlinks/config/zsh/prompt.zsh", config | {"app_tests"}),
            ("symlinks/config/bash/prompt.bash", config | {"app_tests", "lint"}),
            ("symlinks/bash_profile", config | {"app_tests", "lint"}),
            ("symlinks/bashrc", config | {"app_tests", "lint"}),
            ("symlinks/local/bin/code-insiders", config | desktop_scripts | {"rust_checks", "app_tests", "lint"}),
            ("symlinks/config/powershell/Module.psd1",
             config | {"lint", "prompt"}),
            ("symlinks/config/wgetrc", config | {"app_tests"}),
            ("symlinks/config/user-dirs.dirs", config | {"app_tests"}),
            ("symlinks/config/hypr/conf/appearance.lua",
             config | {"desktop_scripts", "lock", "desktop_qml", "lint"}),
            ("symlinks/config/hypr/conf/resize-on-border.lua",
             config | {"desktop_scripts", "lock", "desktop_qml", "lint"}),
            ("symlinks/config/hypr/hyprland.lua", config | {"desktop_qml", "lint"}),
            ("symlinks/config/hypr/hyprlock.conf", config | {"desktop_qml"}),
            ("tests/shell/Test-Prompt.ps1",
             {"lint", "prompt"}),
            ("symlinks/config/hypr/scripts/lock-screen.sh",
             config | {"lint", "desktop_scripts", "lock"}),
            ("tests/desktop/hypr/test_lock_screen.py",
             {"desktop_scripts", "lock", "lint"}),
            ("symlinks/config/hypr/hypridle.conf",
             config | {"desktop_scripts", "lock"}),
            ("symlinks/config/quickshell/helpers/network_helper.py",
             config | {"desktop_scripts", "desktop_python", "desktop_qml", "lint"}),
            ("symlinks/config/quickshell/helpers/stocks.py",
             config | {"desktop_scripts", "desktop_python", "desktop_qml", "lint"}),
            ("tests/desktop/quickshell/python/test_stocks.py",
             {"desktop_scripts", "desktop_python", "lint"}),
            ("symlinks/config/hypr/scripts/screenshot.sh",
             config | {"lint", "desktop_scripts", "lock"}),
            ("symlinks/config/hypr/scripts/choose-editor.sh",
             config | {"lint", "desktop_scripts", "lock"}),
            ("dotfiles.sh", {"lint", "wrapper_linux", "build_linux"}),
            ("dotfiles.ps1", {"lint", "wrapper_windows", "roundtrip_windows", "build_windows"}),
            ("hooks/pre-commit", {"lint", "git_hooks", "hook_inputs"}),
            ("hooks/sensitive-patterns.ini", {"git_hooks", "hook_inputs"}),
            (LINUX + "test-docs.sh", {"lint", "docs_site", "docs_contracts"}),
            (LINUX + "check-doc-links.py", {"lint"}),
            (LINUX + "check-doc-site.py", {"docs_site", "lint"}),
            (LINUX + "docs-publication-base.py", {"docs_site", "lint"}),
            (LINUX + "test-workflows.sh", {"lint"}),
            (LINUX + "install-actionlint.sh", {"lint"}),
            ("tests/desktop/quickshell/test-desktop-qml.sh", {"lint", "desktop_qml"}),
            ("symlinks/config/quickshell/bar/Bar.qml",
             config | {"desktop_scripts", "desktop_python", "desktop_qml"}),
            ("symlinks/config/quickshell/lib/Theme.js",
             config | {"desktop_scripts", "desktop_python", "desktop_qml"}),
            ("tests/desktop/quickshell/python/test_network_helper.py",
             {"desktop_scripts", "desktop_python", "lint"}),
            ("tests/desktop/quickshell/qml/mocks/Quickshell/Io/qmldir",
             {"desktop_qml"}),
            ("tests/vim/test_vim_config.py",
             {"app_tests", "build_linux", "lint"}),
            ("tests/integration/test-arch-acceptance.sh", {"roundtrip_linux", "build_linux", "lint"}),
            ("tests/desktop/quickshell/test-desktop-session.sh", {"desktop_qml", "lint"}),
            ("tests/integration/test-install-uninstall.py",
             {"roundtrip_linux", "roundtrip_windows", "build_linux", "build_windows", "lint"}),
            ("tests/wrapper/Test-ShellWrapper.ps1", {"wrapper_windows", "build_windows", "lint"}),
            ("tests/wrapper/windows/Test-Bootstrap.ps1", {"wrapper_windows", "build_windows", "lint"}),
            ("tests/wrapper/linux/test-bootstrap.sh", {"wrapper_linux", "build_linux", "lint"}),
            ("tests/git/test-git.py", {"app_tests", "app_windows", "build_linux", "build_windows", "lint"}),
            ("tests/shell/test-zsh.py", {"app_tests", "build_linux", "lint"}),
            ("tests/vim/test-vim.py", {"app_tests", "build_linux", "lint"}),
            ("tests/vim/test-nvim.py", {"app_tests", "build_linux", "lint"}),
            ("tests/hooks/hook_inputs_tests/test_allowlists.py", {"hook_inputs", "build_linux", "lint"}),
            ("tests/hooks/git_hook_tests/test_sensitive_scan.py", {"git_hooks", "hook_inputs", "lint"}),
            ("tests/hooks/test-git-hooks.py", {"git_hooks", "hook_inputs", "lint"}),
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
             config | {"prompt", "roundtrip_windows", "build_windows", "lint"}),
            ("symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
             config | {"roundtrip_windows", "build_windows"}),
            ("symlinks/config/systemd/user/quickshell.service", config | desktop_scripts | {"desktop_qml"}),
            ("symlinks/config/systemd/user/clean-home-tmp.timer", config | {"desktop_qml"}),
            ("tests/hooks/test-maintenance.py", {"git_hooks", "hook_inputs", "lint"}),
            ("tests/vim/test-nvim-lock.lua", {"app_tests", "build_linux", "lint"}),
            ("tests/vim/test-nvim-config.lua", {"app_tests", "build_linux", "lint"}),
            ("tests/shell/test-shell-config.py", {"app_tests", "build_linux", "lint"}),
            ("tests/shell/shell_config_tests/test_profile_and_env.py", {"app_tests", "build_linux", "lint"}),
            ("tests/hooks/test-hook-inputs.py", {"hook_inputs", "build_linux", "lint"}),
            ("tests/wrapper/parity-contract.json", {"wrapper_linux", "wrapper_windows", "build_linux", "build_windows"}),
            ("tests/wrapper/test-parity.py", {"wrapper_linux", "wrapper_windows", "build_linux", "build_windows", "lint"}),
            (LINUX + "check-ci-contract.py", {"lint"}),
            (LINUX + "workflow_yaml.py", {"lint"}),
            (LINUX + "test-ci-changes.py", {"lint"}),
            (LINUX + "ci_changes_tests/test_classification.py", {"lint"}),
            (LINUX + "select-mutations.py", {"lint"}),
            (LINUX + "run-mutations.py", {"lint"}),
            (LINUX + "report-mutations.py", {"lint"}),
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
        for path in (".github/workflows/ci.yml", "tests/lib/test-helpers.sh",
                     LINUX + "ci_metadata.py", LINUX + "classify-ci-changes.py",
                     "unknown-build-input"):
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
            (["tests/vim/test-vim.py"], ["Python (Ruff)"], ["vim"]),
            (["tests/vim/test-nvim.py"], ["Python (Ruff)"], ["nvim"]),
            (["tests/git/test-git.py"], ["Python (Ruff)"], ["git"]),
            (["tests/shell/test-zsh.py"], ["Python (Ruff)"], ["zsh"]),
            (["tests/vim/test-nvim-config.lua"], ["Lua (Luacheck)"], ["nvim"]),
            (["tests/shell/test-shell-config.py"], ["Python (Ruff)"], ["zsh"]),
            ([".github/linters/ruff.toml"], ["Python (Ruff)"], []),
            (["symlinks/local/bin/code-insiders"], ["Python (Ruff)"], ["zsh"]),
            ([".github/linters/luacheckrc"], ["Lua (Luacheck)"], []),
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
        self.assertTrue(outputs["run_desktop_scripts"])
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
            "symlinks/config/quickshell/bar/Bar.qml", "README.md",
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
