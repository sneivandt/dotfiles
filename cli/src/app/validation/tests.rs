//! Unit tests for configuration validation tasks.

use super::*;
use std::io::Write;

use crate::infra::config::Diagnostic;
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::test_helpers::{empty_config, make_context, make_linux_context};

#[test]
fn display_diagnostics_formats_severity_and_code() {
    use crate::infra::config::DiagnosticCode;
    use crate::infra::logging::isolated_logger;

    let (logger, _tmp, _guard) = isolated_logger();
    let diagnostics = vec![
        Diagnostic::warning(
            "pkg.toml",
            "git",
            DiagnosticCode::new("package", "empty-name"),
            "name is empty",
        ),
        Diagnostic::error(
            "sym.toml",
            ".bashrc",
            DiagnosticCode::new("symlink", "parent-in-source"),
            "unsafe path",
        ),
    ];

    display_diagnostics(&diagnostics, &logger);
}

#[test]
fn manifest_sync_errors_when_manifest_file_is_missing() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let conf = dir.path().join("conf");
    std::fs::create_dir_all(&conf).expect("conf dir should create");
    std::fs::write(conf.join("symlinks.toml"), "[base]\nsymlinks = []\n")
        .expect("symlinks config should write");

    let ctx = make_linux_context(empty_config(dir.path().to_path_buf()));
    let result = ValidateManifestSync.run(&ctx);

    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("manifest.toml"),
        "missing manifest error should include file path: {msg}"
    );
}

#[test]
fn detects_sh_extension() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let script = dir.path().join("test.sh");
    std::fs::write(&script, "echo hello").expect("write should succeed");

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert_eq!(found.len(), 1);
    assert_eq!(found.first().expect("found 0 should exist"), &script);
}

#[test]
fn detects_shebang_without_extension() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let script = dir.path().join("myscript");
    let mut f = std::fs::File::create(&script).expect("create should succeed");
    f.write_all(b"#!/bin/bash\necho hello")
        .expect("write_all should succeed");

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert_eq!(found.len(), 1);
}

#[test]
fn ignores_non_shell_files() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    std::fs::write(dir.path().join("readme.md"), "# Hello").expect("write should succeed");
    std::fs::write(dir.path().join("data.json"), "{}").expect("write should succeed");

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert!(found.is_empty());
}

#[test]
fn discovers_ps1_files() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let script_path = dir.path().join("test.ps1");
    let module_path = dir.path().join("module.psm1");
    std::fs::write(&script_path, "Write-Host 'hi'").expect("write should succeed");
    std::fs::write(&module_path, "function Test {}").expect("write should succeed");
    std::fs::write(dir.path().join("readme.md"), "# Hello").expect("write should succeed");

    let mut found = Vec::new();
    discover_powershell_scripts(dir.path(), &mut found);
    assert_eq!(found.len(), 2);
}

#[test]
fn discovers_apm_plugin_dirs() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let plugins = dir.path().join("plugins");
    std::fs::create_dir_all(plugins.join("dot-code")).expect("plugin dir should create");
    std::fs::create_dir_all(plugins.join("not-a-plugin")).expect("plain dir should create");
    std::fs::write(plugins.join("dot-code").join("apm.yml"), "name: dot-code\n")
        .expect("apm manifest should write");

    let found = discover_apm_plugin_dirs(&plugins).expect("plugin discovery should succeed");

    assert_eq!(found, vec![plugins.join("dot-code")]);
}

#[test]
fn apm_plugin_validation_runs_pack_dry_run_in_each_plugin() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let plugins = dir.path().join("symlinks").join("apm").join("plugins");
    std::fs::create_dir_all(plugins.join("dot-code")).expect("plugin dir should create");
    std::fs::write(plugins.join("dot-code").join("apm.yml"), "name: dot-code\n")
        .expect("apm manifest should write");

    let mut executor = MockExecutor::new();
    executor.expect_execute().once().returning(|spec| {
        let plugin_dir = spec.working_dir().expect("working directory should be set");
        assert!(
            plugin_dir.ends_with("dot-code"),
            "APM pack should run in the plugin directory, got {}",
            plugin_dir.display()
        );
        assert_eq!(spec.program(), "apm");
        assert_eq!(spec.arguments(), ["pack", "--dry-run", "--verbose"]);
        assert!(!spec.is_checked(), "APM validation should allow non-zero");
        Ok(ExecResult::success(""))
    });

    let ctx = make_context(
        empty_config(dir.path().to_path_buf()),
        crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
        std::sync::Arc::new(executor),
    );

    assert!(ValidateApmPlugins.run(&ctx).is_ok());
}

#[test]
fn discovers_powershell_shebang_without_extension() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let script = dir.path().join("profile-hook");
    std::fs::write(&script, "#!/usr/bin/env pwsh\nWrite-Host 'hi'").expect("write should succeed");

    let mut found = Vec::new();
    discover_powershell_scripts(dir.path(), &mut found);
    assert_eq!(found, vec![script]);
}

#[test]
fn powershell_command_escapes_single_quotes_in_paths() {
    let path = PathBuf::from("C:\\Users\\o'connor\\script.ps1");
    let script = build_psscriptanalyzer_command(&[path]);
    assert!(
        script.contains("C:\\Users\\o''connor\\script.ps1"),
        "single quotes in file paths must be PowerShell-escaped"
    );
}

#[test]
fn shellcheck_command_includes_project_defaults() {
    let args = build_shellcheck_args(&[
        PathBuf::from("dotfiles.sh"),
        PathBuf::from("hooks/pre-commit"),
    ]);

    assert_eq!(
        args,
        vec![
            "--severity=warning".to_string(),
            "--exclude=SC1090,SC1091,SC3043,SC2154".to_string(),
            "--enable=avoid-nullary-conditions".to_string(),
            "dotfiles.sh".to_string(),
            "hooks/pre-commit".to_string(),
        ]
    );
}

#[test]
fn shebang_detects_various_shells() {
    let dir = tempfile::tempdir().expect("tempdir should create");

    for (name, shebang) in [
        ("a", "#!/bin/sh\n"),
        ("b", "#!/bin/bash\n"),
        ("c", "#!/usr/bin/env zsh\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, shebang).expect("write should succeed");
    }

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    // zsh scripts are excluded (shellcheck doesn't support them)
    assert_eq!(found.len(), 2);
}

#[test]
fn shebang_excludes_non_posix_shells() {
    let dir = tempfile::tempdir().expect("tempdir should create");

    // These should NOT be detected as shell scripts
    for (name, shebang) in [
        ("fish_script", "#!/usr/bin/fish\n"),
        ("csh_script", "#!/bin/csh\n"),
        ("tcsh_script", "#!/usr/bin/tcsh\n"),
        ("python_script", "#!/usr/bin/python3\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, shebang).expect("write should succeed");
    }

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert!(
        found.is_empty(),
        "should not match non-POSIX shell shebangs"
    );
}

#[test]
fn shebang_detects_env_wrappers() {
    let dir = tempfile::tempdir().expect("tempdir should create");

    for (name, shebang) in [
        ("a", "#!/usr/bin/env sh\n"),
        ("b", "#!/usr/bin/env bash\n"),
        ("c", "#!/usr/bin/env dash\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, shebang).expect("write should succeed");
    }

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert_eq!(found.len(), 3);
}

#[test]
fn shebang_with_arguments() {
    let dir = tempfile::tempdir().expect("tempdir should create");

    // Shebangs with arguments should still correctly identify the interpreter
    for (name, shebang) in [
        ("a", "#!/bin/sh -e\n"),
        ("b", "#!/bin/bash -x\n"),
        ("c", "#!/usr/bin/env bash -e\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, shebang).expect("write should succeed");
    }

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert_eq!(found.len(), 3, "should detect shell scripts with arguments");
}

#[test]
fn shebang_detects_non_standard_install_paths() {
    let dir = tempfile::tempdir().expect("tempdir should create");

    // Non-standard paths like /usr/local/bin or /opt/homebrew/bin (macOS)
    // must still correctly resolve the interpreter name.
    for (name, shebang) in [
        ("a", "#!/usr/local/bin/bash\n"),
        ("b", "#!/opt/homebrew/bin/bash\n"),
        ("c", "#!/usr/local/bin/sh\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, shebang).expect("write should succeed");
    }

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert_eq!(
        found.len(),
        3,
        "should detect shell scripts with non-standard install paths"
    );
}

#[test]
fn shebang_detects_env_with_flags() {
    let dir = tempfile::tempdir().expect("tempdir should create");

    // `env -S` is used to pass arguments through env on some systems.
    let path = dir.path().join("script");
    std::fs::write(&path, "#!/usr/bin/env -S bash -e\necho hi\n").expect("write should succeed");

    let mut found = Vec::new();
    discover_shell_scripts(dir.path(), &mut found);
    assert_eq!(found.len(), 1, "should detect shell scripts with env -S");
}

#[test]
fn discover_files_with_custom_predicate() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    std::fs::write(dir.path().join("a.txt"), "hello").expect("write should succeed");
    std::fs::write(dir.path().join("b.txt"), "world").expect("write should succeed");
    std::fs::write(dir.path().join("c.md"), "# doc").expect("write should succeed");
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).expect("create_dir should succeed");
    std::fs::write(sub.join("d.txt"), "nested").expect("write should succeed");

    let mut found = Vec::new();
    discover_files(
        dir.path(),
        |p| p.extension().is_some_and(|e| e == "txt"),
        &mut found,
    );
    assert_eq!(found.len(), 3, "should find .txt files recursively");
}

#[test]
fn linter_inputs_include_root_files_then_discovered_scripts() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let root = dir.path();
    std::fs::write(root.join("dotfiles.sh"), "echo hi").expect("root script should write");
    std::fs::create_dir_all(root.join("hooks")).expect("hooks dir should create");
    std::fs::write(root.join("hooks").join("pre-commit.sh"), "echo hook")
        .expect("hook script should write");

    let found = discover_linter_inputs(
        root,
        &["dotfiles.sh", "install.sh"],
        &["hooks", "missing-dir"],
        discover_shell_scripts,
    );

    assert_eq!(
        found,
        vec![
            root.join("dotfiles.sh"),
            root.join("hooks").join("pre-commit.sh"),
        ],
        "root files come first, then scripts from each existing directory"
    );
}

#[test]
fn linter_passes_without_running_the_tool_when_there_is_nothing_to_lint() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let mut executor = MockExecutor::new();
    executor.expect_execute().never();
    let ctx = make_context(
        empty_config(dir.path().to_path_buf()),
        crate::infra::platform::Platform::detect(),
        std::sync::Arc::new(executor),
    );

    let result = run_linter(
        &ctx,
        "shellcheck",
        "shellcheck",
        "shell scripts",
        &[],
        |_| Vec::new(),
    )
    .expect("empty input should pass");

    assert!(
        matches!(result, crate::engine::TaskResult::CheckPassed),
        "an empty input set is a passing check, not a failure"
    );
}

#[test]
fn linter_failure_reports_the_display_name_not_the_executable() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let mut executor = MockExecutor::new();
    executor
        .expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::failure("some finding", "", Some(1))));
    let ctx = make_context(
        empty_config(dir.path().to_path_buf()),
        crate::infra::platform::Platform::detect(),
        std::sync::Arc::new(executor),
    );

    let error = run_linter(
        &ctx,
        "pwsh",
        "PSScriptAnalyzer",
        "PowerShell scripts",
        &[PathBuf::from("a.ps1")],
        |_| vec!["-NoProfile".to_owned()],
    )
    .expect_err("a failing linter should fail the task");

    assert!(
        error.to_string().contains("PSScriptAnalyzer"),
        "failure should name the linter, not the host executable: {error}"
    );
}
