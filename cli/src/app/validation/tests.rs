//! Unit tests for configuration validation tasks.

use super::*;
use crate::infra::config::Diagnostic;
use crate::infra::exec::{ExecError, ExecResult, MockExecutor};
use crate::test_helpers::{empty_config, make_context, make_linux_context};
use crate::{domains::files::config::chmod::ChmodEntry, infra::ConfigHandle};

#[test]
fn display_diagnostics_formats_severity_and_code() {
    use crate::infra::config::DiagnosticCode;
    use crate::infra::logging::MsgKind;

    use crate::test_helpers::CapturingOutput;

    let output = CapturingOutput::default();
    display_diagnostics(&[], &output);
    assert!(output.messages().is_empty());
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
        output.messages(),
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
    let ctx = make_linux_context(root, None);

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
    let ctx = make_linux_context(root, None);

    assert!(store.symlinks.get().is_empty());
    assert!(store.chmod.get().is_empty());
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
            dir.path().to_path_buf(),
            None,
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
fn powershell_command_escapes_single_quotes_in_root() {
    let script = build_psscriptanalyzer_command(std::path::Path::new("C:\\Users\\o'connor"));
    assert!(script.ends_with("-Root 'C:\\Users\\o''connor'"));
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
            "fixture-root".into(),
            None,
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
            std::sync::Arc::new(executor),
        );
        let result = run_linter(
            &ctx,
            "PSScriptAnalyzer",
            crate::infra::exec::CommandSpec::new("pwsh").args(&["-NoProfile", "-Command", "lint"]),
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

#[test]
fn configured_source_validation_is_inapplicable_without_sources() {
    let config = empty_config("/fixture".into());
    let ctx = make_linux_context(config.root.clone(), config.overlay.clone());
    let task = ValidateSymlinkSources::new(ConfigHandle::new(config));
    assert!(!task.should_run(&ctx));
}

#[test]
fn linter_tasks_delegate_to_embedded_scripts() {
    let cases: [(&dyn Task, &str); 2] = [
        (&RunShellcheck, "shellcheck"),
        (&RunPSScriptAnalyzer, "pwsh"),
    ];
    for (task, tool) in cases {
        let root = PathBuf::from("fixture root [literal]");
        let expected_root = root.clone();
        let mut executor = MockExecutor::new();
        executor.expect_which().return_const(true);
        executor.expect_execute().once().return_once(move |spec| {
            if tool == "shellcheck" {
                assert_eq!(spec.program(), "sh");
                assert_eq!(
                    spec.arguments(),
                    [
                        "-c",
                        SHELLCHECK_SCRIPT,
                        "dotfiles-shellcheck",
                        "--root",
                        &expected_root.to_string_lossy()
                    ]
                );
            } else {
                assert_eq!(spec.program(), "pwsh");
                assert_eq!(
                    spec.arguments(),
                    [
                        "-NoProfile",
                        "-Command",
                        &build_psscriptanalyzer_command(&expected_root)
                    ]
                );
            }
            assert!(!spec.is_checked());
            Ok(ExecResult::success(""))
        });
        let ctx = make_context(
            root,
            None,
            crate::infra::platform::Platform::detect(),
            std::sync::Arc::new(executor),
        );
        assert!(matches!(
            task.run(&ctx).unwrap(),
            crate::engine::TaskResult::CheckPassed
        ));
    }
}
