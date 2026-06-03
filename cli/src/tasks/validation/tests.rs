//! Unit tests for configuration validation tasks.

use super::*;
use std::io::Write;

use crate::tasks::test_helpers::{empty_config, make_linux_context};

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
