use super::*;
use crate::infra::env::MapEnv;

#[test]
fn inherited_overlay_marker_requires_child_lock_and_non_empty_marker() {
    use std::ffi::OsStr;
    assert_eq!(
        inherited_overlay_marker(true, true, Some(OsStr::new("tok"))),
        Some(OsStr::new("tok"))
    );
    assert_eq!(
        inherited_overlay_marker(false, true, Some(OsStr::new("tok"))),
        None
    );
    assert_eq!(
        inherited_overlay_marker(true, false, Some(OsStr::new("tok"))),
        None
    );
    assert_eq!(
        inherited_overlay_marker(true, true, Some(OsStr::new(""))),
        None
    );
    assert_eq!(inherited_overlay_marker(true, true, None), None);
}

#[test]
fn explicit_root_is_canonicalized_before_environment_selection() {
    let temp_dir = tempfile::tempdir().unwrap();
    let env = MapEnv::new().with("DOTFILES_ROOT", "/environment-root");
    for root in [temp_dir.path().to_path_buf(), temp_dir.path().join(".")] {
        let result = resolve_root_from_dir(Some(&root), None, &env).unwrap();
        assert_eq!(
            result,
            crate::infra::fs::canonicalize(temp_dir.path()).unwrap()
        );
        assert_eq!(
            resolve_root_path(Some(&root)).unwrap(),
            result,
            "standalone discovery must resolve the same explicit root"
        );
    }
}

#[test]
fn only_parent_commands_acquire_the_repository_lock() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let (log, _logs, _guard) = crate::infra::logging::isolated_logger();
    let log = Arc::new(log);
    for elevated in [false, true] {
        for guarded in [false, true] {
            let mut global =
                super::super::tests::global(if elevated { &["--elevated-child"] } else { &[] });
            global.root = Some(root.path().to_path_buf());
            let env = if guarded {
                MapEnv::new().with(super::super::reexec::REEXEC_GUARD_VAR, "")
            } else {
                MapEnv::new()
            };
            let env = env
                .with("HOME", root.path())
                .with("USERPROFILE", root.path());
            let runtime = RuntimePolicy::new(&global, false, env.into_handle(), true, true);
            let lock = CommandRunner::acquire_run_lock(&runtime, root.path(), &log).unwrap();
            assert_eq!(
                lock.is_some(),
                !elevated,
                "elevated={elevated}, guarded={guarded}"
            );
        }
    }
}

#[test]
fn delegated_overlay_is_not_a_new_saved_selection() {
    let root = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(root.path()).unwrap();
    let previous = tempfile::tempdir().unwrap();
    let one_run = tempfile::tempdir().unwrap();
    overlay_resolution::persist(root.path(), previous.path()).unwrap();
    let config_path = repo.path().join("config");
    let before = std::fs::read(&config_path).unwrap();
    let conf = root.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    for file in crate::app::config::REQUIRED_CONFIG_FILES {
        std::fs::write(conf.join(file), "").unwrap();
    }

    let mut global = super::super::tests::global(&["--elevated-child"]);
    global.root = Some(root.path().to_path_buf());
    global.overlay = Some(one_run.path().to_path_buf());
    let env = MapEnv::new()
        .with("HOME", root.path())
        .with("USERPROFILE", root.path());
    let runtime = RuntimePolicy::new(&global, false, env.into_handle(), true, true);
    for _ in 0..2 {
        let runner = CommandRunner::new_with_profile_prompt(
            &runtime,
            &Arc::new(Logger::test()),
            &crate::engine::CancellationToken::new(),
            CommandStartup::resolve(&runtime, &Logger::test()).unwrap(),
            || Ok("base".into()),
        )
        .unwrap();
        assert_eq!(runner.ctx.overlay(), Some(one_run.path()));
        assert_eq!(std::fs::read(&config_path).unwrap(), before);
    }
}

#[test]
fn startup_saves_selections_only_after_configuration_loads() {
    let root = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(root.path()).unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let previous = root.path().join("previous-overlay");
    std::fs::create_dir(&previous).unwrap();
    overlay_resolution::persist(root.path(), &previous).unwrap();
    let git_config = repo.path().join("config");
    let original = std::fs::read(&git_config).unwrap();
    let conf = root.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    for file in crate::app::config::REQUIRED_CONFIG_FILES {
        std::fs::write(conf.join(file), "").unwrap();
    }
    let invalid = conf.join("symlinks.toml");
    std::fs::write(&invalid, "[invalid").unwrap();

    let mut global = super::super::tests::global(&["--dry-run"]);
    global.root = Some(root.path().to_path_buf());
    global.overlay = Some(overlay.path().to_path_buf());
    let env = MapEnv::new()
        .with("HOME", root.path())
        .with("USERPROFILE", root.path());
    let runtime = RuntimePolicy::new(&global, false, env.into_handle(), true, true);
    let log = Arc::new(Logger::test());
    let token = crate::engine::CancellationToken::new();
    let start = || {
        CommandRunner::new_with_profile_prompt(
            &runtime,
            &log,
            &token,
            CommandStartup::resolve(&runtime, &log)?,
            || Ok("desktop".into()),
        )
    };

    let error = start().expect_err("invalid TOML should stop startup");
    assert!(format!("{error:#}").contains("symlinks.toml"), "{error:#}");
    assert_eq!(std::fs::read(&git_config).unwrap(), original);

    std::fs::write(invalid, "").unwrap();
    let runner = start().expect("startup should succeed after repairing the TOML");
    assert_eq!(runner.ctx.overlay(), Some(overlay.path()));
    assert_eq!(
        overlay_resolution::read_persisted(root.path())
            .unwrap()
            .as_deref(),
        Some(overlay.path())
    );
    assert_eq!(
        profiles::read_persisted(root.path()).unwrap().as_deref(),
        Some("desktop")
    );

    drop(runner);
    let saved = std::fs::read(&git_config).unwrap();
    global.overlay = None;
    let overrides = MapEnv::new()
        .with("HOME", root.path())
        .with("USERPROFILE", root.path())
        .with("DOTFILES_PROFILE", "base")
        .with("DOTFILES_OVERLAY", &previous);
    let unattended = RuntimePolicy::new(&global, false, overrides.into_handle(), false, false);
    let (overridden, recovery_pending) =
        CommandRunner::for_check(&unattended, &log, &token).unwrap();
    assert!(!recovery_pending);
    assert_eq!(overridden.ctx.overlay(), Some(previous.as_path()));
    assert_eq!(overridden.config_handle().get().profile.name, "base");
    assert_eq!(std::fs::read(&git_config).unwrap(), saved);
}

#[test]
fn self_update_child_reuses_handed_off_prompt_profile_without_repeating_header() {
    use super::super::reexec::{
        REEXEC_GUARD_VAR, SELF_UPDATE_HEADER_EMITTED_VAR, SELF_UPDATE_PROFILE_PROMPTED_VAR,
        SELF_UPDATE_PROFILE_VAR, SELF_UPDATE_REEXEC_GUARD_VAR,
    };

    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let conf = root.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    for file in crate::app::config::REQUIRED_CONFIG_FILES {
        std::fs::write(conf.join(file), "").unwrap();
    }
    let home = tempfile::tempdir().unwrap();

    // Parent resolves profile and emits the header before self-update, without persisting early.
    let mut global = super::super::tests::global(&["--dry-run"]);
    global.root = Some(root.path().to_path_buf());
    let base_env = MapEnv::new()
        .with("HOME", home.path())
        .with("USERPROFILE", home.path());
    let parent_runtime =
        RuntimePolicy::new(&global, false, base_env.clone().into_handle(), true, true);
    let parent_log = Arc::new(Logger::test());
    let parent_startup =
        CommandStartup::for_mutation_with_profile_prompt(&parent_runtime, &parent_log, || {
            Ok("desktop".into())
        })
        .unwrap();
    let expected_header = format!(
        "{} · dry run · desktop profile · ",
        parent_log.command_title()
    );
    assert_eq!(
        parent_log
            .captured_lines()
            .iter()
            .filter(|line| line.starts_with(&expected_header))
            .count(),
        1,
        "{:?}",
        parent_log.captured_lines()
    );
    assert_eq!(profiles::read_persisted(root.path()).unwrap(), None);

    // Restarted child reuses the handed-off profile without prompting or repeating the header,
    // while inheriting the parent's run lock, and persists it only after config loads.
    let (_continuation, mode) = parent_startup
        .run_lock
        .as_ref()
        .unwrap()
        .prepare_restart()
        .unwrap();
    let lease =
        crate::infra::run_lock::RestartLease::new(root.path(), &base_env, Platform::detect(), mode)
            .unwrap();
    let child_env = base_env
        .with(REEXEC_GUARD_VAR, lease.marker().unwrap())
        .with(SELF_UPDATE_REEXEC_GUARD_VAR, "1")
        .with(SELF_UPDATE_HEADER_EMITTED_VAR, "1")
        .with(SELF_UPDATE_PROFILE_VAR, "desktop")
        .with(SELF_UPDATE_PROFILE_PROMPTED_VAR, "1");
    let child_runtime = RuntimePolicy::new(&global, false, child_env.into_handle(), true, true);
    let child_log = Arc::new(Logger::test());
    let token = crate::engine::CancellationToken::new();
    let child_startup =
        CommandStartup::for_mutation_with_profile_prompt(&child_runtime, &child_log, || {
            panic!("self-update child must reuse the parent's prompted profile")
        })
        .unwrap();
    let runner =
        CommandRunner::from_startup(&child_runtime, &child_log, &token, child_startup).unwrap();
    drop(parent_startup);
    assert_eq!(runner.config_handle().get().profile.name, "desktop");
    assert!(
        child_log
            .captured_lines()
            .iter()
            .all(|line| !line.starts_with(&expected_header)),
        "{:?}",
        child_log.captured_lines()
    );
    assert_eq!(
        profiles::read_persisted(root.path()).unwrap().as_deref(),
        Some("desktop")
    );
}

#[test]
fn self_update_profile_handoff_respects_guard_elevation_and_prompt_flag() {
    use super::super::reexec::{
        REEXEC_GUARD_VAR, SELF_UPDATE_PROFILE_PROMPTED_VAR, SELF_UPDATE_PROFILE_VAR,
        SELF_UPDATE_REEXEC_GUARD_VAR,
    };

    let root = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(root.path()).unwrap();
    repo.config()
        .unwrap()
        .set_str("dotfiles.profile", "desktop")
        .unwrap();
    let home = tempfile::tempdir().unwrap();
    let base_env = MapEnv::new()
        .with("HOME", home.path())
        .with("USERPROFILE", home.path());
    let global = super::super::tests::global(&[]);

    // Non-prompted handoff reuses the profile without treating it as a prompt.
    let non_prompt_env = base_env
        .clone()
        .with(REEXEC_GUARD_VAR, "1")
        .with(SELF_UPDATE_REEXEC_GUARD_VAR, "1")
        .with(SELF_UPDATE_PROFILE_VAR, "base");
    let non_prompt_runtime =
        RuntimePolicy::new(&global, false, non_prompt_env.into_handle(), false, true);
    let (non_prompt_profile, non_prompt_source) = resolve_profile(
        &non_prompt_runtime,
        root.path(),
        Platform::detect(),
        None,
        &Logger::test(),
        || panic!("handed-off profile should not prompt"),
    )
    .unwrap();
    assert_eq!(non_prompt_profile.name, "base");
    assert_eq!(non_prompt_source, profiles::ProfileSource::Argument);

    // Prompt flag without a handed-off profile does not rewrite an explicit --profile source.
    let arg_global = super::super::tests::global(&["--profile", "base"]);
    let orphan_prompt_env = base_env
        .clone()
        .with(REEXEC_GUARD_VAR, "1")
        .with(SELF_UPDATE_REEXEC_GUARD_VAR, "1")
        .with(SELF_UPDATE_PROFILE_PROMPTED_VAR, "1");
    let orphan_runtime = RuntimePolicy::new(
        &arg_global,
        false,
        orphan_prompt_env.into_handle(),
        false,
        true,
    );
    let (arg_profile, arg_source) = resolve_profile(
        &orphan_runtime,
        root.path(),
        Platform::detect(),
        None,
        &Logger::test(),
        || panic!("explicit profile should not prompt"),
    )
    .unwrap();
    assert_eq!(arg_profile.name, "base");
    assert_eq!(arg_source, profiles::ProfileSource::Argument);

    // Self-update profile handoff is ignored outside a non-elevated self-update child.
    for (flags, guard) in [(vec![], false), (vec!["--elevated-child"], true)] {
        let g = super::super::tests::global(&flags);
        let env = base_env
            .clone()
            .with(SELF_UPDATE_PROFILE_VAR, "base")
            .with(SELF_UPDATE_PROFILE_PROMPTED_VAR, "1");
        let env = if guard {
            env.with(REEXEC_GUARD_VAR, "1")
                .with(SELF_UPDATE_REEXEC_GUARD_VAR, "1")
        } else {
            env
        };
        let rt = RuntimePolicy::new(&g, false, env.into_handle(), false, true);
        let (fallback_profile, fallback_source) = resolve_profile(
            &rt,
            root.path(),
            Platform::detect(),
            None,
            &Logger::test(),
            || panic!("persisted profile should resolve without prompting"),
        )
        .unwrap();
        assert_eq!(fallback_profile.name, "desktop", "{flags:?} guard={guard}");
        assert_eq!(
            fallback_source,
            profiles::ProfileSource::Repository,
            "{flags:?} guard={guard}"
        );
    }
}

#[test]
fn repository_mutation_startup_emits_or_suppresses_profileless_header() {
    use super::super::reexec::{
        REEXEC_GUARD_VAR, SELF_UPDATE_HEADER_EMITTED_VAR, SELF_UPDATE_REEXEC_GUARD_VAR,
    };

    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let conf = root.path().join("conf");
    std::fs::create_dir(&conf).unwrap();
    for file in crate::app::config::REQUIRED_CONFIG_FILES {
        std::fs::write(conf.join(file), "").unwrap();
    }
    let home = tempfile::tempdir().unwrap();
    let base_env = MapEnv::new()
        .with("HOME", home.path())
        .with("USERPROFILE", home.path());

    for (dry_run, expected_dry_run) in [(false, false), (true, true)] {
        let mut global = super::super::tests::global(if dry_run { &["--dry-run"] } else { &[] });
        global.root = Some(root.path().to_path_buf());
        let runtime =
            RuntimePolicy::new(&global, false, base_env.clone().into_handle(), false, true);
        let log = Arc::new(Logger::test());
        let startup = CommandStartup::for_repository_mutation(&runtime, &log).unwrap();
        let lines = log.captured_lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(
            lines[0].contains(" · dry run · "),
            expected_dry_run,
            "{lines:?}"
        );
        assert!(!lines[0].contains(" profile "), "{lines:?}");
        if dry_run {
            let (_continuation, mode) = startup
                .run_lock
                .as_ref()
                .unwrap()
                .prepare_restart()
                .unwrap();
            let lease = crate::infra::run_lock::RestartLease::new(
                root.path(),
                &base_env,
                Platform::detect(),
                mode,
            )
            .unwrap();
            let child_env = base_env
                .clone()
                .with(REEXEC_GUARD_VAR, lease.marker().unwrap())
                .with(SELF_UPDATE_REEXEC_GUARD_VAR, "1")
                .with(SELF_UPDATE_HEADER_EMITTED_VAR, "1");
            let child_runtime =
                RuntimePolicy::new(&global, false, child_env.into_handle(), false, true);
            let child_log = Arc::new(Logger::test());
            let _child_startup =
                CommandStartup::for_repository_mutation(&child_runtime, &child_log).unwrap();
            assert!(
                child_log.captured_lines().is_empty(),
                "{:?}",
                child_log.captured_lines()
            );
        }
        drop(startup);
    }
}

#[test]
fn overlay_tasks_use_the_context_overlay_and_startup_script_snapshot() {
    for overlay in [None, Some(PathBuf::from("/fixture-overlay"))] {
        let mut config = crate::test_helpers::empty_config("/fixture-repo".into());
        config.overlay = overlay.clone();
        config
            .scripts
            .push(crate::domains::overlay::config::scripts::ScriptEntry {
                name: "fixture".into(),
                path: "scripts/fixture.sh".into(),
                description: None,
                check_remove: false,
            });
        let (ctx, log) =
            crate::test_helpers::make_static_context(config.root.clone(), config.overlay.clone());
        let runner = CommandRunner {
            run_lock: None,
            ctx,
            log,
            store: ConfigStore::from_config(config),
        };
        let tasks = runner.overlay_script_tasks();
        let selectors: Vec<_> = tasks.iter().map(|task| task.selector()).collect();
        assert_eq!(
            selectors,
            if overlay.is_some() {
                vec!["script-fixture"]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn environment_root_is_canonicalized_and_must_exist() {
    let fixture = tempfile::tempdir_in(".").unwrap();
    let root = fixture.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let env = MapEnv::new().with("DOTFILES_ROOT", &root);
    assert_eq!(
        resolve_root_from_dir(None, None, &env).unwrap(),
        crate::infra::fs::canonicalize(&root).unwrap()
    );
    let missing = MapEnv::new().with("DOTFILES_ROOT", root.join("missing"));
    assert!(
        resolve_root_from_dir(None, None, &missing).is_err(),
        "an invalid environment root must not produce an empty configuration"
    );
}

#[cfg(unix)]
#[test]
fn relative_environment_root_creates_resolvable_home_symlinks() {
    use crate::domains::files::config::symlinks::Symlink;
    use crate::domains::files::symlinks::InstallSymlinks;
    use crate::test_helpers::{assert_task_changed, make_linux_context};

    let fixture = tempfile::tempdir_in(".").unwrap();
    let fixture_root = crate::infra::fs::canonicalize(fixture.path()).unwrap();
    let root = fixture_root.join("repo");
    let home = fixture_root.join("home");
    std::fs::create_dir_all(root.join("symlinks")).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::write(root.join("symlinks/example"), "managed content").unwrap();
    let cwd = std::env::current_dir().unwrap();
    let relative_root = root.strip_prefix(&cwd).unwrap();
    let env = MapEnv::new().with("DOTFILES_ROOT", relative_root);
    let resolved = resolve_root_from_dir(None, Some(&cwd), &env).unwrap();
    let ctx = make_linux_context(resolved.clone(), None).with_home(home.clone());
    let task = InstallSymlinks::new(ConfigHandle::new(vec![Symlink {
        source: "example".into(),
        target: None,
        allow_external_target: false,
        origin: Some(resolved),
    }]));

    assert_task_changed(&task.run(&ctx).unwrap());

    assert_eq!(
        std::fs::read_to_string(home.join(".example")).unwrap(),
        "managed content",
        "link payloads must not resolve the repository relative to the user's home"
    );
}

#[test]
fn resolve_root_errors_when_not_in_repo() {
    let root = tempfile::tempdir().unwrap();
    let error = resolve_root_from_dir(None, Some(root.path()), &MapEnv::new()).unwrap_err();
    assert!(error.to_string().contains("cannot determine dotfiles root"));
}

#[test]
fn nested_executable_uses_its_own_checkout_for_installed_and_cargo_layouts() {
    let outer = tempfile::tempdir().unwrap();
    let nested = outer.path().join("worktrees/desktop");
    for (root, label) in [
        (outer.path(), "outer settings"),
        (&nested, "nested settings"),
    ] {
        std::fs::create_dir_all(root.join("conf")).unwrap();
        std::fs::create_dir(root.join("symlinks")).unwrap();
        std::fs::write(root.join("conf/selection"), label).unwrap();
    }
    for bin in ["bin", "cli/target/debug"] {
        let dir = nested.join(bin);
        std::fs::create_dir_all(&dir).unwrap();
        let resolved = root_from_executable(&dir.join("dotfiles"))
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read(resolved.join("conf/selection")).unwrap(),
            b"nested settings"
        );
    }
}

#[test]
fn executable_discovery_requires_both_managed_directories() {
    for incomplete in ["conf", "symlinks"] {
        let root = tempfile::tempdir().unwrap();
        let checkout = root.path().join("worktrees/fixture");
        std::fs::create_dir_all(checkout.join(incomplete)).unwrap();
        let bin = checkout.join("bin");
        std::fs::create_dir(&bin).unwrap();
        assert!(
            root_from_executable(&bin.join("dotfiles"))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn elevated_child_prefers_forwarded_selections_to_stale_restart_environment() {
    use super::super::reexec::{
        REEXEC_GUARD_VAR, REPOSITORY_OVERLAY_VAR, REPOSITORY_PROFILE_VAR,
        REPOSITORY_REEXEC_GUARD_VAR,
    };

    let root = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let old_overlay = tempfile::tempdir().unwrap();
    let mut global = super::super::tests::global(&["--elevated-child", "--profile", "base"]);
    global.root = Some(root.path().to_path_buf());
    global.overlay = Some(overlay.path().to_path_buf());
    let env = MapEnv::new()
        .with("HOME", root.path())
        .with("USERPROFILE", root.path())
        .with(REEXEC_GUARD_VAR, "inherited-marker")
        .with(REPOSITORY_REEXEC_GUARD_VAR, "1")
        .with(REPOSITORY_OVERLAY_VAR, old_overlay.path())
        .with(REPOSITORY_PROFILE_VAR, "desktop");
    let runtime = RuntimePolicy::new(&global, false, env.into_handle(), false, true);
    assert!(runtime.repository_child);
    let log = Logger::test();
    let startup = CommandStartup::resolve(&runtime, &log).unwrap();
    assert_eq!(
        startup.overlay,
        Some((overlay.path().to_owned(), OverlaySource::Argument))
    );
    let (profile, source) = resolve_profile(
        &runtime,
        root.path(),
        Platform::detect(),
        Some(overlay.path()),
        &log,
        || panic!("forwarded profile should not prompt"),
    )
    .unwrap();
    assert_eq!(profile.name, "base");
    assert_eq!(source, profiles::ProfileSource::Argument);
}

#[test]
fn profile_precedence_and_prompt_policy_use_the_same_runtime_environment() {
    use profiles::ProfileSource::{Argument, Environment, Repository};

    let root = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(root.path()).unwrap();
    repo.config()
        .unwrap()
        .set_str("dotfiles.profile", "base")
        .unwrap();
    let log = Logger::test();

    for (flags, env_profile, expected) in [
        (vec![], None, Some(("base", Repository))),
        (vec![], Some("desktop"), Some(("desktop", Environment))),
        (
            vec!["--profile", "base"],
            Some("desktop"),
            Some(("base", Argument)),
        ),
        (vec!["--profile", "missing"], Some("desktop"), None),
        (vec![], Some("missing"), None),
    ] {
        let global = super::super::tests::global(&flags);
        let env = env_profile.map_or_else(
            || MapEnv::new().with("HOME", root.path()),
            |name| {
                MapEnv::new()
                    .with("HOME", root.path())
                    .with("DOTFILES_PROFILE", name)
            },
        );
        let runtime = RuntimePolicy::new(&global, false, env.into_handle(), false, true);
        let profile = resolve_profile(
            &runtime,
            root.path(),
            Platform::detect(),
            None,
            &log,
            || panic!("unexpected profile prompt"),
        );
        assert_eq!(
            profile.ok().map(|(profile, source)| (profile.name, source)),
            expected.map(|(name, source)| (name.to_owned(), source)),
            "{flags:?} {env_profile:?}"
        );
    }

    repo.config().unwrap().remove("dotfiles.profile").unwrap();
    let global = super::super::tests::global(&[]);
    let runtime = RuntimePolicy::new(&global, false, MapEnv::new().into_handle(), false, true);
    let error = resolve_profile(
        &runtime,
        root.path(),
        Platform::detect(),
        None,
        &log,
        || panic!("unexpected profile prompt"),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("profile selection is required in non-interactive mode")
    );
}
