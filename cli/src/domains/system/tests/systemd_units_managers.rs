use super::*;

#[test]
fn applicability_depends_on_platform_configuration_and_ci() {
    for (os, configured, ci, expected) in [
        (Os::Windows, true, false, false),
        (Os::Linux, false, false, false),
        (Os::Linux, true, true, false),
        (Os::Linux, true, false, true),
    ] {
        let task = if configured {
            systemd_task("dunst.service", UnitScope::User, true)
        } else {
            ConfigureSystemd::new(ConfigHandle::new(Vec::new()))
        };
        let ctx = ContextBuilder::new("/tmp".into())
            .os(os)
            .which(true)
            .ci(ci)
            .build();
        assert_eq!(
            task.should_run(&ctx),
            expected,
            "{os:?}, configured={configured}, ci={ci}"
        );
    }
}

#[test]
fn missing_systemctl_is_checked_at_run_time() {
    let task = systemd_task("dunst.service", UnitScope::User, true);
    let ctx = make_linux_context("/tmp".into(), None);
    assert!(task.should_run(&ctx));
    for dry_run in [false, true] {
        assert!(
            matches!(
                task.run(&ctx.with_dry_run(dry_run)).unwrap(),
                TaskResult::Skipped { reason, kind: SkipKind::UnmetWork }
                    if reason == "systemctl unavailable"
            ),
            "a required tool missing after prerequisites leaves work unmet"
        );
    }
}

#[test]
fn contradictory_units_are_rejected_before_any_command_including_preview() {
    let task = ConfigureSystemd::new(ConfigHandle::new(
        [true, false]
            .map(|enabled| SystemdUnit {
                name: "audit.service".into(),
                scope: UnitScope::User,
                graphical: false,
                enabled,
            })
            .to_vec(),
    ));
    let ctx = context(MockExecutor::new());
    for dry_run in [false, true] {
        assert!(
            format!("{:#}", task.run(&ctx.with_dry_run(dry_run)).unwrap_err())
                .contains("conflicting systemd")
        );
    }
}

#[test]
fn run_calls_daemon_reload_before_enabling_unit() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let task = systemd_task("dunst.service", UnitScope::User, true);
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    let installed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&installed);
    mock.expect_which()
        .once()
        .with(mockall::predicate::eq("systemctl"))
        .returning(move |_| observed.load(Ordering::SeqCst));
    for (cmd, result) in [
        (user_manager_probe(), ExecResult::success("")),
        (user_reload(), ExecResult::success("")),
        (
            systemctl(&["--user", "is-enabled", "dunst.service"]),
            disabled_result(),
        ),
        (
            systemctl(&["--user", "enable", "--now", "dunst.service"]),
            ExecResult::success(""),
        ),
    ] {
        mock.expect_execute()
            .once()
            .in_sequence(&mut seq)
            .with(mockall::predicate::eq(cmd))
            .return_once(|_| Ok(result));
    }
    let ctx = context(mock);
    assert!(
        task.should_run(&ctx),
        "assessment must not skip systemctl provided by a prerequisite"
    );
    installed.store(true, Ordering::SeqCst);

    let result = task.run(&ctx).unwrap();
    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
        "expected one changed action after daemon-reload + enable, got {result:?}"
    );
}

#[test]
fn unavailable_wsl_manager_is_checked_at_run_time() {
    let task = systemd_task("example.service", UnitScope::User, true);
    let ctx = manager_context(
        Platform::new_wsl(),
        true,
        Some((
            systemctl(&["is-system-running"]),
            Ok(ExecResult::failure("offline", "", Some(1))),
        )),
    );

    assert!(task.should_run(&ctx));
    assert!(
        matches!(task.run(&ctx).unwrap(), TaskResult::NotApplicable(reason) if reason == "systemd unavailable")
    );
}

#[test]
fn wsl_systemd_availability_accepts_running_or_degraded_managers() {
    for (name, observed, expected) in [
        ("running", ExecResult::success("running\n"), true),
        (
            "degraded",
            ExecResult::failure("  degraded\n", "", Some(1)),
            true,
        ),
        (
            "offline",
            ExecResult::failure("offline\n", "", Some(1)),
            false,
        ),
    ] {
        let executor = script().command(systemctl(&["is-system-running"]), Ok(observed));
        let ctx = make_context(
            "/fixture".into(),
            None,
            Platform::new_wsl(),
            Arc::new(executor),
        );
        assert_eq!(systemd_available(&ctx).unwrap(), expected, "{name}");
    }
}

#[test]
fn wsl_manager_inspection_errors_are_not_unavailable_states() {
    let task = systemd_task("example.service", UnitScope::User, true);
    for (stdout, stderr) in [
        ("unknown\n", "Failed to query system state: Access denied"),
        ("", "Failed to connect to bus: Permission denied"),
    ] {
        for dry_run in [false, true] {
            let executor = script().command(
                systemctl(&["is-system-running"]),
                Ok(ExecResult::failure(stdout, stderr, Some(1))),
            );
            let ctx = make_context(
                "/fixture".into(),
                None,
                Platform::new_wsl(),
                Arc::new(executor),
            )
            .with_dry_run(dry_run);
            let error = task.run(&ctx).unwrap_err();
            assert!(format!("{error:#}").contains(stderr), "{error:#}");
        }
    }
}

#[test]
fn run_skips_daemon_reload_in_dry_run() {
    let task = systemd_task("dunst.service", UnitScope::User, true);
    let executor = script()
        .command(user_manager_probe(), Ok(ExecResult::success("")))
        .command(
            systemctl(&["--user", "is-enabled", "dunst.service"]),
            Ok(disabled_result()),
        );
    let result = task.run(&context(executor).with_dry_run(true)).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
}

#[test]
fn daemon_reload_failures_abort_user_and_system_tasks() {
    for (scope, name, probe, observed, reload, message) in [
        (
            UnitScope::User,
            "dunst.service",
            user_manager_probe(),
            ExecResult::success(""),
            user_reload(),
            "reloading user systemd daemon",
        ),
        (
            UnitScope::System,
            "sshd.service",
            systemctl(&["is-enabled", "sshd.service"]),
            disabled_result(),
            CommandSpec::new("sudo").args(&["systemctl", "daemon-reload"]),
            "reloading system systemd daemon",
        ),
    ] {
        let failure = ExecError::spawn(
            reload.program().to_string_lossy(),
            std::io::Error::other("reload failed"),
        );
        let executor = script()
            .command(probe, Ok(observed))
            .command(reload, Err(failure));
        let error = systemd_task(name, scope, true)
            .run(&context(executor))
            .unwrap_err();
        assert!(error.to_string().contains(message), "{name}: {error:#}");
    }
}

#[cfg(unix)]
#[test]
fn unavailable_manager_and_arch_chroot_enable_units_offline() {
    let offline = ExecResult::failure("", "Failed to connect to bus: No medium found", Some(1));
    let chroot = MapEnv::new().with(
        crate::infra::provisioning::ENV_VAR,
        crate::infra::provisioning::ARCH_CHROOT,
    );
    for ctx in [
        context(script().command(user_manager_probe(), Ok(offline))),
        context(script()).with_env(chroot.into_handle()), // No user bus probe in a chroot.
    ] {
        let home = tempfile::tempdir().unwrap();
        let unit_dir = home.path().join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        let name = "clean-home-tmp.timer";
        std::fs::write(unit_dir.join(name), "[Install]\nWantedBy=timers.target\n").unwrap();
        let ctx = ctx.with_home(home.path().to_path_buf());
        let result = systemd_task(name, UnitScope::User, true).run(&ctx).unwrap();
        assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
        assert_eq!(
            std::fs::read_link(unit_dir.join("timers.target.wants").join(name)).unwrap(),
            unit_dir.join(name)
        );
    }
}

#[cfg(unix)]
#[test]
fn rejected_user_manager_probe_preserves_login_links_until_access_recovers() {
    let home = tempfile::tempdir().unwrap();
    let name = "fixture-user-bus.service";
    let user_dir = home.path().join(".config/systemd/user");
    let wants = user_dir.join("default.target.wants");
    std::fs::create_dir_all(&wants).unwrap();
    let unit = user_dir.join(name);
    std::fs::write(&unit, "[Install]\nWantedBy=default.target\n").unwrap();
    let link = wants.join(name);
    std::os::unix::fs::symlink(&unit, &link).unwrap();
    let task = systemd_task(name, UnitScope::User, false);
    let probe = user_manager_probe();

    for diagnostic in [
        "Failed to connect to bus: Permission denied",
        "Failed to connect to user scope bus via local transport: Connection refused",
        "Failed to get environment: Message recipient disconnected",
    ] {
        for dry_run in [true, false] {
            let executor = script().command(
                probe.clone(),
                Ok(ExecResult::failure("PRIVATE=value", diagnostic, Some(1))),
            );
            let ctx = context(executor)
                .with_home(home.path().to_path_buf())
                .with_dry_run(dry_run);
            let error = task.run(&ctx).unwrap_err().to_string();
            assert!(error.contains(diagnostic), "{error}");
            assert!(!error.contains("PRIVATE"));
            assert_eq!(std::fs::read_link(&link).unwrap(), unit);
        }
    }

    let ctx = context(script().command(probe, Ok(ExecResult::success("PRIVATE=value"))))
        .with_home(home.path().to_path_buf());
    assert!(user_manager_available(&ctx, task.config.get()).unwrap());
}

#[test]
fn user_manager_absence_diagnostics_are_recognized_in_the_c_locale() {
    for error in [
        "Failed to connect to bus: No such file or directory",
        "Failed to connect to bus: No medium found",
        "Failed to connect to user scope bus via local transport: $DBUS_SESSION_BUS_ADDRESS and $XDG_RUNTIME_DIR not defined (consider using --machine=...)",
    ] {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .with(mockall::predicate::eq(user_manager_probe()))
            .returning(move |_| Ok(ExecResult::failure("", error, Some(1))));
        let task = systemd_task("fixture.service", UnitScope::User, true);
        assert!(!user_manager_available(&context(mock), task.config.get()).unwrap());
    }
}
