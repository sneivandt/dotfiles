//! Unit tests for configuration validation tasks.

use super::*;
use crate::infra::config::Diagnostic;
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::test_helpers::{empty_config, make_context, make_linux_context};
use crate::{domains::files::config::chmod::ChmodEntry, infra::ConfigHandle};

#[test]
fn configuration_check_reports_unsupported_xdg_layouts() {
    use crate::domains::files::config::symlinks::config_home_diagnostic;
    use crate::infra::config::Severity;
    use crate::infra::env::MapEnv;

    let fixture = tempfile::tempdir().unwrap();
    let mut config = empty_config(fixture.path().into());
    let sources = fixture.path().join("symlinks/config/git");
    std::fs::create_dir_all(&sources).unwrap();
    std::fs::write(sources.join("config"), "").unwrap();
    config.symlinks = vec![crate::test_helpers::symlink("config/git/config", None)];
    let home = fixture.path().join("home");
    for value in [
        None,
        Some(std::ffi::OsString::new()),
        Some(home.join(".config").into_os_string()),
    ] {
        let env = value.map_or_else(MapEnv::new, |value| {
            MapEnv::new().with("XDG_CONFIG_HOME", value)
        });
        assert!(config_home_diagnostic(&config.symlinks, &home, &env).is_none());
    }
    let env = MapEnv::new().with("XDG_CONFIG_HOME", fixture.path().join("custom"));
    let diagnostic = config_home_diagnostic(&config.symlinks, &home, &env).unwrap();
    assert_eq!(
        diagnostic.code.to_string(),
        "symlink.unsupported-config-home"
    );
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.source, "symlinks.toml");
    assert_eq!(diagnostic.item, "XDG_CONFIG_HOME");
    assert!(diagnostic.message.contains("or unset"));
    let ctx = make_linux_context(fixture.path().into(), None)
        .with_home(home.clone())
        .with_env(env.into_handle());
    let error = ValidateConfigWarnings::new(ConfigHandle::new(config))
        .run(&ctx)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "configuration check failed: 1 issue found"
    );
    assert!(
        config_home_diagnostic(
            &[crate::test_helpers::symlink(
                "config/file",
                Some("custom/file")
            )],
            &home,
            &MapEnv::new().with("XDG_CONFIG_HOME", "custom"),
        )
        .is_none()
    );
}

#[test]
fn required_configuration_passes_only_when_all_files_are_present() {
    use crate::engine::TaskResult;

    let fixture = tempfile::tempdir().unwrap();
    let ctx = make_linux_context(fixture.path().into(), None);
    let conf = fixture.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    for file in crate::app::config::REQUIRED_CONFIG_FILES {
        std::fs::write(conf.join(file), "").unwrap();
    }
    assert!(matches!(
        ValidateConfigFiles.run(&ctx).unwrap(),
        TaskResult::CheckPassed
    ));
    let missing = crate::app::config::REQUIRED_CONFIG_FILES[0];
    std::fs::remove_file(conf.join(missing)).unwrap();
    assert_eq!(
        ValidateConfigFiles.run(&ctx).unwrap_err().to_string(),
        "1 required config file(s) missing"
    );
}

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
            (MsgKind::Warn, "found 2 configuration issues:".into()),
            (
                MsgKind::WarnContinuation,
                "  [warn] pkg.toml [git] (package.empty-name): name is empty".into(),
            ),
            (
                MsgKind::WarnContinuation,
                "  [err] sym.toml [.bashrc] (symlink.parent-in-source): unsafe path".into(),
            ),
        ]
    );
}

#[test]
fn startup_diagnostic_continuations_keep_stderr_and_raw_jsonl_facts() {
    use crate::infra::config::DiagnosticCode;
    use crate::infra::logging::{OutputMode, isolated_logger};
    let diagnostics = [Diagnostic::warning(
        "fixture.toml",
        "item",
        DiagnosticCode::new("fixture", "invalid"),
        "fix the value",
    )];
    for mode in [OutputMode::Human, OutputMode::Quiet, OutputMode::Jsonl] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(false);
        log.set_output_mode(mode);
        display_diagnostics(&diagnostics, &log);
        let writes = log.captured_output();
        let item = "  [warn] fixture.toml [item] (fixture.invalid): fix the value";
        if mode == OutputMode::Jsonl {
            let values: Vec<serde_json::Value> = writes
                .iter()
                .map(|(stderr, line)| {
                    assert!(!stderr);
                    serde_json::from_str(line).unwrap()
                })
                .collect();
            assert_eq!(values.len(), 2);
            assert_eq!(values[1]["event"], "warn");
            assert_eq!(values[1]["text"], item);
        } else {
            assert_eq!(
                writes,
                [
                    (true, "warn  found 1 configuration issue:\n".into()),
                    (true, format!("{item}\n"))
                ]
            );
        }
        let stored = std::fs::read_to_string(log.log_path().unwrap()).unwrap();
        assert!(stored.contains(item.trim_start()));
        assert!(!stored.contains("warn  "));
    }
}

#[test]
fn completed_validation_diagnostics_keep_their_continuations_in_both_modes() {
    use crate::infra::config::DiagnosticCode;
    use crate::infra::logging::{
        ActionCounts, BufferedLog, TaskEntry, TaskStatus, TaskVisibility, isolated_logger,
    };
    use std::sync::Arc;
    let diagnostics = [Diagnostic::warning(
        "fixture.toml",
        "item",
        DiagnosticCode::new("fixture", "invalid"),
        "fix the value",
    )];
    for verbose in [false, true] {
        let (mut log, _tmp, _guard) = isolated_logger();
        log.set_verbose(verbose);
        let log = Arc::new(log);
        let buffer = BufferedLog::new(Arc::clone(&log));
        display_diagnostics(&diagnostics, &buffer);
        log.record_task(TaskEntry::new(
            "validation",
            "Validation",
            TaskStatus::Failed,
            None,
            ActionCounts::default(),
            TaskVisibility::Visible,
        ));
        buffer.flush_and_complete("validation", "Validation");
        let writes = log.captured_output();
        assert!(
            writes
                .iter()
                .any(|(stderr, text)| *stderr && text == "  warn  found 1 configuration issue:\n")
        );
        assert!(writes.iter().any(|(stderr, text)| *stderr
            && text == "    [warn] fixture.toml [item] (fixture.invalid): fix the value\n"));
    }
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

#[cfg(unix)]
#[test]
fn configured_source_validation_inspects_each_terminal_glob_match_before_passing() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let sources = root.join("symlinks");
    let tools = sources.join("tools");
    let matched = tools.join("editor");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("valid"), "managed").unwrap();
    let outside = root.join("custom");
    std::fs::write(&outside, "custom").unwrap();
    let mut config = empty_config(root.to_path_buf());
    // Not selected by the current profile, but check still covers this declaration.
    config.validation_symlinks = vec![crate::test_helpers::symlink("tools/*", None)];
    let task = ValidateSymlinkSources::new(ConfigHandle::new(config));
    let ctx = make_linux_context(root.to_path_buf(), None).with_home(root.join("home"));
    std::os::unix::fs::symlink("../missing", &matched).unwrap();
    assert_eq!(
        task.run(&ctx).unwrap_err().to_string(),
        "1 configured source(s) missing"
    );
    std::fs::remove_file(&matched).unwrap();
    std::os::unix::fs::symlink(&outside, &matched).unwrap();
    let error = task.run(&ctx).unwrap_err();
    assert!(error.to_string().contains("outside its symlinks directory"));
    assert_eq!(std::fs::read_link(&matched).unwrap(), outside);
    std::fs::remove_file(&matched).unwrap();
    std::os::unix::fs::symlink("valid", &matched).unwrap();
    assert!(matches!(
        task.run(&ctx).unwrap(),
        crate::engine::TaskResult::CheckPassed
    ));
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "custom");
    assert!(!ctx.home().exists());
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
            allow_external_target: false,
            origin: Some(overlay.clone()),
        },
        Symlink {
            source: "overlay-only.sh".into(),
            target: None,
            allow_external_target: false,
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

#[cfg(unix)]
#[test]
fn broken_apm_manifest_is_not_a_passing_empty_inventory() {
    let fixture = tempfile::tempdir().unwrap();
    let plugins = fixture.path().join("plugins");
    let plugin = plugins.join("custom");
    std::fs::create_dir_all(&plugin).unwrap();
    let manifest = plugin.join("apm.yml");
    std::os::unix::fs::symlink("local-policy.yml", &manifest).unwrap();
    assert!(discover_apm_plugin_dirs(&plugins).is_err());
    assert_eq!(
        std::fs::read_link(&manifest).unwrap(),
        std::path::Path::new("local-policy.yml")
    );
    std::fs::write(plugin.join("local-policy.yml"), "name: custom\n").unwrap();
    assert_eq!(discover_apm_plugin_dirs(&plugins).unwrap(), [plugin]);
}

#[test]
fn apm_validation_checks_each_plugin_and_aggregates_findings() {
    use crate::infra::logging::MsgKind;
    use crate::test_helpers::CapturingOutput;

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
                        ExecResult::failure(
                            format!("{name}: invalid manifest"),
                            format!("{name}: missing skill"),
                            Some(1),
                        )
                    } else {
                        ExecResult::success("")
                    })
                });
        }
        let output = std::sync::Arc::new(CapturingOutput::default());
        let ctx = make_context(
            dir.path().to_path_buf(),
            None,
            crate::infra::platform::Platform::new(crate::infra::platform::Os::Linux, false),
            std::sync::Arc::new(executor),
        )
        .with_log(std::sync::Arc::<CapturingOutput>::clone(&output));

        let result = ValidateApmPlugins.run(&ctx);
        let errors: Vec<String> = output
            .messages()
            .into_iter()
            .filter_map(|(kind, msg)| (kind == MsgKind::Error).then_some(msg))
            .collect();
        if has_findings {
            assert_eq!(
                result.unwrap_err().to_string(),
                "2 APM plugin(s) failed validation",
                "findings must not stop validation of later plugins"
            );
            assert_eq!(
                errors,
                [
                    format!(
                        "APM plugin validation failed: {}",
                        plugins.join("dot-a").display()
                    ),
                    "dot-a: invalid manifest".into(),
                    "dot-a: missing skill".into(),
                    format!(
                        "APM plugin validation failed: {}",
                        plugins.join("dot-c").display()
                    ),
                    "dot-c: invalid manifest".into(),
                    "dot-c: missing skill".into(),
                ]
            );
        } else {
            assert!(matches!(
                result.unwrap(),
                crate::engine::TaskResult::CheckPassed
            ));
            assert!(errors.is_empty());
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
