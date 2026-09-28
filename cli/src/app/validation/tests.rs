//! Unit tests for configuration validation tasks.

use super::*;
use crate::infra::config::Diagnostic;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::test_helpers::{empty_config, make_context, make_linux_context};
use crate::{domains::files::config::chmod::ChmodEntry, infra::ConfigHandle};

#[test]
fn display_diagnostics_formats_severity_and_code() {
    use crate::infra::config::DiagnosticCode;
    use crate::infra::logging::{MsgKind, Output};

    #[derive(Default)]
    struct CapturingOutput(std::sync::Mutex<Vec<(MsgKind, String)>>);

    impl Output for CapturingOutput {
        fn emit(&self, kind: MsgKind, message: std::borrow::Cow<'_, str>) {
            self.0.lock().unwrap().push((kind, message.into_owned()));
        }
    }

    let output = CapturingOutput::default();
    display_diagnostics(&[], &output);
    assert!(output.0.lock().unwrap().is_empty());
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

    display_diagnostics(&diagnostics, &output);
    assert_eq!(
        *output.0.lock().unwrap(),
        [
            (MsgKind::Warn, "found 2 configuration diagnostic(s):".into()),
            (
                MsgKind::Warn,
                "  [warn] pkg.toml [git] (package.empty-name): name is empty".into(),
            ),
            (
                MsgKind::Warn,
                "  [err] sym.toml [.bashrc] (symlink.parent-in-source): unsafe path".into(),
            ),
        ]
    );
}

#[test]
fn configured_source_validation_rejects_missing_chmod_source() {
    let dir = tempfile::tempdir().expect("tempdir should create");
    let root = dir.path().to_path_buf();
    let mut config = empty_config(root.clone());
    config.validation_chmod = vec![ChmodEntry::new("755", "config/missing.sh")];
    let task = ValidateSymlinkSources::new(ConfigHandle::new(config));
    let ctx = make_linux_context(empty_config(root));

    let error = task
        .run(&ctx)
        .expect_err("missing chmod source must fail validation");

    assert!(
        error.to_string().contains("configured source"),
        "error should identify the configured source failure: {error}"
    );
}

#[test]
fn configured_source_validation_keeps_overlay_origins_and_unfiltered_sources() {
    use crate::domains::files::config::symlinks::Symlink;

    let dir = tempfile::tempdir().expect("fixture directory");
    let root = dir.path().join("main");
    let overlay = dir.path().join("overlay");
    std::fs::create_dir_all(root.join("symlinks")).unwrap();
    std::fs::create_dir_all(overlay.join("symlinks/skills")).unwrap();
    std::fs::write(overlay.join("symlinks/skills/tool"), "").unwrap();
    std::fs::write(overlay.join("symlinks/overlay-only.sh"), "").unwrap();

    let mut config = empty_config(root.clone());
    config.overlay = Some(overlay.clone());
    config.validation_symlinks = vec![
        Symlink {
            source: "skills/*".into(),
            target: None,
            origin: Some(overlay.clone()),
        },
        Symlink {
            source: "overlay-only.sh".into(),
            target: None,
            origin: Some(overlay.clone()),
        },
    ];
    config.validation_chmod = vec![ChmodEntry::new("755", "overlay-only.sh")];
    let store = crate::app::config::store::ConfigStore::from_config(config);
    let task = ValidateSymlinkSources::new(store.aggregate.clone());
    let ctx = make_linux_context(empty_config(root));

    assert!(store.symlinks.read().is_empty());
    assert!(store.chmod.read().is_empty());
    assert!(
        task.should_run(&ctx),
        "unfiltered sources must still be checked"
    );
    assert!(matches!(
        task.run(&ctx).unwrap(),
        crate::engine::TaskResult::CheckPassed
    ));

    std::fs::remove_file(overlay.join("symlinks/overlay-only.sh")).unwrap();
    let error = task
        .run(&ctx)
        .expect_err("both missing source references must be reported");
    assert_eq!(error.to_string(), "2 configured source(s) missing");
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
    let manifest_path = dir.path().join("module.psd1");
    std::fs::write(&script_path, "Write-Host 'hi'").expect("write should succeed");
    std::fs::write(&module_path, "function Test {}").expect("write should succeed");
    std::fs::write(&manifest_path, "@{}").expect("write should succeed");
    std::fs::write(dir.path().join("readme.md"), "# Hello").expect("write should succeed");

    let mut found = Vec::new();
    discover_powershell_scripts(dir.path(), &mut found);
    found.sort();
    let mut expected = vec![script_path, module_path, manifest_path];
    expected.sort();
    assert_eq!(found, expected);
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
fn apm_validation_checks_each_plugin_and_aggregates_findings() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let plugins = dir.path().join("symlinks").join("apm").join("plugins");
    for name in ["dot-c", "dot-a", "dot-b"] {
        std::fs::create_dir_all(plugins.join(name)).unwrap();
        std::fs::write(
            plugins.join(name).join("apm.yml"),
            format!("name: {name}\n"),
        )
        .unwrap();
    }

    for has_findings in [false, true] {
        let mut executor = MockExecutor::new();
        executor
            .expect_which()
            .with(mockall::predicate::eq("apm"))
            .once()
            .return_const(true);
        let mut sequence = mockall::Sequence::new();
        for name in ["dot-a", "dot-b", "dot-c"] {
            let plugin = plugins.join(name);
            executor
                .expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .returning(move |spec| {
                    assert_eq!(spec.working_dir(), Some(plugin.as_path()));
                    assert_eq!(spec.program(), "apm");
                    assert_eq!(spec.arguments(), ["pack", "--dry-run", "--verbose"]);
                    assert!(!spec.is_checked(), "APM validation should allow non-zero");
                    Ok(if has_findings && name != "dot-b" {
                        ExecResult::failure("invalid manifest", "", Some(1))
                    } else {
                        ExecResult::success("")
                    })
                });
        }
        let ctx = make_context(
            empty_config(dir.path().to_path_buf()),
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
            std::sync::Arc::new(executor),
        );

        let result = ValidateApmPlugins.run(&ctx);
        if has_findings {
            assert_eq!(
                result.unwrap_err().to_string(),
                "2 APM plugin(s) failed validation",
                "findings must not stop validation of later plugins"
            );
        } else {
            assert!(matches!(
                result.unwrap(),
                crate::engine::TaskResult::CheckPassed
            ));
        }
    }
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
fn powershell_command_fails_when_analyzer_cannot_run() {
    let script = build_psscriptanalyzer_command(&[PathBuf::from("script.ps1")]);

    assert!(script.contains("$ErrorActionPreference = 'Stop'"));
    assert!(script.contains("PSScriptAnalyzer module is not installed"));
    assert!(script.contains("Import-Module PSScriptAnalyzer -Force -ErrorAction Stop"));
    assert!(
        script.contains("Invoke-ScriptAnalyzer -Path $_ -Severity Warning,Error -ErrorAction Stop")
    );
    assert!(
        !script.contains("skipping"),
        "missing analyzer must not be reported as a clean skip"
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
fn shell_discovery_checks_each_interpreter_and_extension_override() {
    for (name, contents, accepted) in [
        ("sh", "#!/bin/sh\n", true),
        ("bash", "#!/bin/bash\n", true),
        ("ksh", "#!/bin/ksh\n", true),
        ("env-sh", "#!/usr/bin/env sh\n", true),
        ("env-bash", "#!/usr/bin/env bash\n", true),
        ("env-dash", "#!/usr/bin/env dash\n", true),
        ("sh-args", "#!/bin/sh -e\n", true),
        ("bash-args", "#!/bin/bash -x\n", true),
        ("env-args", "#!/usr/bin/env bash -e\n", true),
        ("local-bash", "#!/usr/local/bin/bash\n", true),
        ("homebrew", "#!/opt/homebrew/bin/bash\n", true),
        ("local-sh", "#!/usr/local/bin/sh\n", true),
        ("env-split", "#!/usr/bin/env -S bash -e\n", true),
        ("exe", "#!/usr/bin/env.exe bash.exe\n", true),
        ("tabs", "#!\t/usr/bin/env\tbash\n", true),
        ("no-newline", "#!/bin/bash", true),
        ("zsh", "#!/usr/bin/env zsh\n", false),
        ("fish", "#!/usr/bin/fish\n", false),
        ("csh", "#!/bin/csh\n", false),
        ("tcsh", "#!/usr/bin/tcsh\n", false),
        ("python", "#!/usr/bin/python3\n", false),
        ("suffix", "#!/bin/notbash\n", false),
        ("env-missing", "#!/usr/bin/env -S\n", false),
        ("empty", "#!\n", false),
        ("not-first-line", "\n#!/bin/bash\n", false),
        ("excluded.zsh", "#!/bin/bash\n", false),
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let script = dir.path().join(name);
        std::fs::write(&script, contents).unwrap();
        let mut found = Vec::new();
        discover_shell_scripts(dir.path(), &mut found);
        assert_eq!(
            found,
            if accepted { vec![script] } else { vec![] },
            "{name}: {contents:?}"
        );
    }
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
    found.sort();
    let mut expected = vec![
        dir.path().join("a.txt"),
        dir.path().join("b.txt"),
        sub.join("d.txt"),
    ];
    expected.sort();
    assert_eq!(
        found, expected,
        "find only matching files, including nested ones"
    );
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
        &["dotfiles.sh"],
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
        |_| panic!("empty inputs must not even build a linter invocation"),
    )
    .expect("empty input should pass");

    assert!(
        matches!(result, crate::engine::TaskResult::CheckPassed),
        "an empty input set is a passing check, not a failure"
    );
}

#[test]
fn linter_execution_preserves_command_contract_and_failure_kind() {
    for (name, response) in [
        ("success", Ok(ExecResult::success(""))),
        ("findings", Ok(ExecResult::failure("finding", "", Some(1)))),
        (
            "spawn",
            Err(ExecError::spawn(
                "pwsh",
                std::io::Error::other("fixture spawn failure"),
            )),
        ),
        (
            "cancelled",
            Err(ExecError::Cancelled {
                command: "pwsh".into(),
                result: ExecResult::success(""),
            }),
        ),
    ] {
        let mut executor = MockExecutor::new();
        executor.expect_execute().once().return_once(move |spec| {
            assert_eq!(spec.program(), "pwsh");
            assert_eq!(spec.arguments(), ["-NoProfile", "-Command", "lint"]);
            assert_eq!(spec.working_dir(), None);
            assert!(
                !spec.is_checked(),
                "linter exit codes are interpreted by the task"
            );
            response
        });
        let ctx = make_context(
            empty_config("fixture-root".into()),
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
            std::sync::Arc::new(executor),
        );
        let files = [PathBuf::from("a.ps1"), PathBuf::from("module.psm1")];
        let result = run_linter(
            &ctx,
            "pwsh",
            "PSScriptAnalyzer",
            "PowerShell scripts",
            &files,
            |inputs| {
                assert_eq!(inputs, files);
                vec!["-NoProfile".into(), "-Command".into(), "lint".into()]
            },
        );
        match name {
            "success" => assert!(matches!(
                result.unwrap(),
                crate::engine::TaskResult::CheckPassed
            )),
            "findings" => assert_eq!(
                result.unwrap_err().to_string(),
                "PSScriptAnalyzer found issues"
            ),
            "spawn" => assert!(matches!(
                result.unwrap_err().downcast_ref::<ExecError>(),
                Some(ExecError::Spawn { .. })
            )),
            "cancelled" => assert!(matches!(
                result.unwrap_err().downcast_ref::<ExecError>(),
                Some(ExecError::Cancelled { .. })
            )),
            _ => panic!("unknown linter case: {name}"),
        }
    }
}
