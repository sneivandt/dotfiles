use super::*;

/// Build a context backed by `MockExecutor` for `run()` tests.
fn make_systemd_context(mut executor: MockExecutor) -> Context {
    executor
        .expect_which()
        .with(mockall::predicate::eq("systemctl"))
        .return_const(true);
    context(executor)
}

#[test]
fn system_unit_elevation_follows_enablement_and_runtime_state() {
    for (name, enabled, current, expected) in [
        ("sshd.service", true, disabled_result(), true),
        (
            "NetworkManager.service",
            true,
            ExecResult::success("enabled\n"),
            false,
        ),
        (
            "dhcpcd.service",
            false,
            ExecResult::success("enabled\n"),
            true,
        ),
    ] {
        let mut executor = script().command(systemctl(&["is-enabled", name]), Ok(current));
        if !expected {
            executor = executor.command(
                runtime_probe(&UnitScope::System, name),
                Ok(ExecResult::success("ActiveState=active\n")),
            );
        }
        let task = systemd_task(name, UnitScope::System, enabled);
        assert_eq!(
            crate::engine::requires_elevation(&task, &context(executor)),
            expected,
            "{name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn arch_chroot_system_units_change_only_persistent_enablement_and_retry_without_a_bus() {
    let name = "fixture.timer";
    for (enabled, action, before, after) in [
        (
            true,
            "enable",
            disabled_result(),
            ExecResult::success("enabled"),
        ),
        (
            false,
            "disable",
            ExecResult::success("enabled"),
            disabled_result(),
        ),
    ] {
        let task = systemd_task(name, UnitScope::System, enabled);
        let probe = || systemctl(&["--root=/", "is-enabled", name]);
        let change = || {
            CommandSpec::new("sudo")
                .args(&["systemctl", "--root=/", action, name])
                .unchecked()
        };
        let executor = script()
            .command(probe(), Ok(before.clone())) // elevation
            .command(probe(), Ok(before.clone())) // preview
            .command(probe(), Ok(before.clone()))
            .command(
                change(),
                Ok(ExecResult::failure("", "fixture interruption", Some(1))),
            )
            .command(probe(), Ok(before))
            .command(change(), Ok(ExecResult::success("")))
            .command(probe(), Ok(after.clone())) // readiness
            .command(probe(), Ok(after)); // repeat apply
        let ctx = context(executor).with_env(
            MapEnv::new()
                .with(
                    crate::infra::provisioning::ENV_VAR,
                    crate::infra::provisioning::ARCH_CHROOT,
                )
                .into_handle(),
        );
        assert!(task.needs_elevation(&ctx));
        assert_eq!(
            crate::test_helpers::task_batch(&task.run(&ctx.with_dry_run(true)).unwrap())
                .changed_count(),
            1
        );
        assert_eq!(
            crate::test_helpers::task_batch(&task.run(&ctx).unwrap()).failed_count(),
            1
        );
        assert_eq!(
            crate::test_helpers::task_batch(&task.run(&ctx).unwrap()).changed_count(),
            1
        );
        assert!(crate::engine::requirement::ordering_requirements_satisfied(
            &ctx,
            &task.ordering_requirements()
        ));
        assert_eq!(
            crate::test_helpers::task_batch(&task.run(&ctx).unwrap()).already_ok_count(),
            1
        );
    }
}

#[test]
fn run_does_not_reload_enabled_system_scope_units() {
    let name = "NetworkManager.service";
    let task = systemd_task(name, UnitScope::System, true);
    let mut executor = script();
    for _ in 0..2 {
        executor = executor
            .command(
                systemctl(&["is-enabled", name]),
                Ok(ExecResult::success("enabled\n")),
            )
            .command(
                runtime_probe(&UnitScope::System, name),
                Ok(ExecResult::success("ActiveState=active\n")),
            );
    }
    let result = task.run(&context(executor)).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&result).already_ok_count(),
        1
    );
}

#[test]
fn previously_condition_skipped_system_timer_requires_elevation_for_retry() {
    let name = "fstrim.timer";
    let task = systemd_task(name, UnitScope::System, true);
    let executor = script()
        .command(
            systemctl(&["is-enabled", name]),
            Ok(ExecResult::success("enabled\n")),
        )
        .command(
            runtime_probe(&UnitScope::System, name),
            Ok(ExecResult::success(
                "ActiveState=inactive\nConditionResult=no\n",
            )),
        );
    assert!(task.needs_elevation(&context(executor)));
}

fn user_unit_probe(
    executor: ScriptedExecutor,
    name: &str,
    dry_run: bool,
    runtime: &str,
) -> ScriptedExecutor {
    let mut executor = executor.command(user_manager_probe(), Ok(ExecResult::success("")));
    if !dry_run {
        executor = executor.command(user_reload(), Ok(ExecResult::success("")));
    }
    executor
        .command(
            systemctl(&["--user", "is-enabled", name]),
            Ok(ExecResult::success("enabled\n")),
        )
        .command(
            runtime_probe(&UnitScope::User, name),
            Ok(ExecResult::success(runtime)),
        )
}

#[test]
fn systemd_retries_a_previously_skipped_start_and_then_converges() {
    let name = "example.service";
    let task = systemd_task(name, UnitScope::User, true);
    let skipped = "ActiveState=inactive\nConditionResult=no\n";
    let preview = context(user_unit_probe(script(), name, true, skipped)).with_dry_run(true);
    let preview = task.run(&preview).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&preview).changed_count(), 1);

    let executor = user_unit_probe(script(), name, false, skipped).command(
        systemctl(&["--user", "enable", "--now", name]),
        Ok(ExecResult::success("")),
    );
    let ctx = context(user_unit_probe(
        executor,
        name,
        false,
        "ActiveState=active\n",
    ));
    let applied = task.run(&ctx).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&applied).changed_count(), 1);
    let repeated = task.run(&ctx).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&repeated).already_ok_count(),
        1
    );
}

#[test]
fn first_login_service_retries_pending_work_and_converges_after_marker_removal() {
    use crate::infra::provisioning::DEFERRED_EXTENSIONS_MARKER;

    let home = tempfile::tempdir().unwrap();
    let marker = home.path().join(DEFERRED_EXTENSIONS_MARKER);
    let name = "dotfiles-first-login.service";
    let task = systemd_task(name, UnitScope::User, true);
    let skipped = "ActiveState=inactive\nType=oneshot\nResult=success\nConditionResult=no\n";
    let unstarted = "ActiveState=inactive\nType=oneshot\nResult=success\nConditionResult=yes\nExecMainStartTimestampMonotonic=0\n";
    let completed = "ActiveState=inactive\nType=oneshot\nResult=success\nConditionResult=yes\nExecMainStartTimestampMonotonic=123\n";
    let executor = user_unit_probe(script(), name, false, skipped);
    let executor = user_unit_probe(executor, name, false, unstarted);
    let executor = user_unit_probe(executor, name, true, skipped);
    let executor = user_unit_probe(executor, name, false, completed).command(
        systemctl(&["--user", "enable", "--now", "--no-block", name]),
        Ok(ExecResult::success("")),
    );
    let executor = user_unit_probe(executor, name, false, completed);
    let ctx = context(user_unit_probe(executor, name, false, unstarted))
        .with_home(home.path().to_path_buf());

    for _ in 0..2 {
        let result = task.run(&ctx).unwrap();
        assert_eq!(
            crate::test_helpers::task_batch(&result).already_ok_count(),
            1
        );
    }

    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "pending").unwrap();
    let preview = task.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&preview).changed_count(), 1);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "pending");

    let applied = task.run(&ctx).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&applied).changed_count(), 1);
    // Model successful completion by the service's ExecStartPost.
    std::fs::remove_file(&marker).unwrap();
    let repeated = task.run(&ctx).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&repeated).already_ok_count(),
        1
    );

    std::fs::create_dir(&marker).unwrap();
    let invalid = task.run(&ctx).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&invalid).failed_count(), 1);
    assert!(marker.is_dir());
}

#[test]
fn live_thunar_service_restarts_for_drift_and_leaves_matching_settings_alone() {
    let name = "thunar-view-settings.service";
    let task = systemd_task(name, UnitScope::User, true);
    let probe = |executor, dry_run, settings| {
        user_unit_probe(
            executor,
            name,
            dry_run,
            "ActiveState=active\nType=oneshot\n",
        )
        .command(
            CommandSpec::new("/bin/sh")
                .arg(std::path::Path::new("/tmp").join(".config/Thunar/managed-view-settings.sh"))
                .arg("check")
                .unchecked(),
            Ok(settings),
        )
    };

    let old = ExecResult::failure("", "", Some(1));
    let current = ExecResult::success("");
    let executor = probe(script(), true, old.clone());
    let executor = probe(executor, false, old)
        .command(
            systemctl(&["--user", "enable", name]),
            Ok(ExecResult::success("")),
        )
        .command(
            systemctl(&["--user", "restart", name]),
            Ok(ExecResult::success("")),
        );
    let executor = probe(executor, false, current);
    let ctx = context(probe(
        executor,
        false,
        ExecResult::failure("", "no bus", Some(2)),
    ))
    .with_home("/tmp".into());
    for dry_run in [true, false] {
        let result = task.run(&ctx.with_dry_run(dry_run)).unwrap();
        assert_eq!(crate::test_helpers::task_batch(&result).changed_count(), 1);
    }
    let converged = task.run(&ctx).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&converged).already_ok_count(),
        1
    );
    let failed_probe = task.run(&ctx).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&failed_probe).failed_count(),
        1
    );
}

#[test]
fn run_reloads_then_changes_system_enablement_with_sudo() {
    for (name, enabled, action, observed) in [
        ("sshd.service", true, "enable", disabled_result()),
        (
            "dhcpcd.service",
            false,
            "disable",
            ExecResult::success("enabled\n"),
        ),
    ] {
        let executor = script()
            .command(systemctl(&["is-enabled", name]), Ok(observed.clone()))
            .command(
                CommandSpec::new("sudo").args(&["systemctl", "daemon-reload"]),
                Ok(ExecResult::success("")),
            )
            .command(systemctl(&["is-enabled", name]), Ok(observed));
        let executor = if enabled {
            executor
        } else {
            executor.command(
                CommandSpec::new("sudo")
                    .args(&["systemctl", "disable", "--runtime", name])
                    .unchecked(),
                Ok(ExecResult::success("")),
            )
        };
        let executor = executor.command(
            CommandSpec::new("sudo")
                .args(&["systemctl", action, "--now", name])
                .unchecked(),
            Ok(ExecResult::success("")),
        );
        let result = systemd_task(name, UnitScope::System, enabled)
            .run(&context(executor))
            .unwrap();
        assert_eq!(
            crate::test_helpers::task_batch(&result).changed_count(),
            1,
            "{name}"
        );
    }
}

#[test]
fn stale_active_system_unit_reloads_without_restarting_it() {
    let name = "test.service";
    let observed = "ActiveState=active\nNeedDaemonReload=yes\n";
    let refreshed = "ActiveState=active\nNeedDaemonReload=no\n";
    let enablement = || systemctl(&["is-enabled", name]);
    let executor = script()
        .command(enablement(), Ok(ExecResult::success("enabled")))
        .command(
            runtime_probe(&UnitScope::System, name),
            Ok(ExecResult::success(observed)),
        )
        .command(
            CommandSpec::new("sudo").args(&["systemctl", "daemon-reload"]),
            Ok(ExecResult::success("")),
        )
        .command(enablement(), Ok(ExecResult::success("enabled")))
        .command(
            runtime_probe(&UnitScope::System, name),
            Ok(ExecResult::success(refreshed)),
        );
    let result = systemd_task(name, UnitScope::System, true)
        .run(&context(executor))
        .unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&result).already_ok_count(),
        1
    );
}

#[test]
fn runtime_drift_plans_elevation_respects_dry_run_and_converges() {
    use mockall::predicate::eq;
    use std::sync::atomic::{AtomicBool, Ordering};
    let name = "test.service";
    let sudo = |args: &[&str]| CommandSpec::new("sudo").arg("systemctl").args(args);
    for enabled in [true, false] {
        let task = systemd_task(name, UnitScope::System, enabled);
        let active = Arc::new(AtomicBool::new(!enabled));
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .with(eq(systemctl(&["is-enabled", name])))
            .returning(move |_| {
                Ok(if enabled {
                    ExecResult::success("enabled\n")
                } else {
                    disabled_result()
                })
            });
        let observed = Arc::clone(&active);
        mock.expect_execute()
            .with(eq(runtime_probe(&UnitScope::System, name)))
            .returning(move |_| {
                Ok(ExecResult::success(if observed.load(Ordering::SeqCst) {
                    "ActiveState=active\n"
                } else {
                    "ActiveState=inactive\n"
                }))
            });
        let mut mutations = mockall::Sequence::new();
        mock.expect_execute()
            .once()
            .in_sequence(&mut mutations)
            .with(eq(sudo(&["daemon-reload"])))
            .returning(|_| Ok(ExecResult::success("")));
        let applied = Arc::clone(&active);
        if !enabled {
            mock.expect_execute()
                .once()
                .in_sequence(&mut mutations)
                .with(eq(sudo(&["disable", "--runtime", name]).unchecked()))
                .returning(|_| Ok(ExecResult::success("")));
        }
        let action = if enabled { "enable" } else { "disable" };
        mock.expect_execute()
            .once()
            .in_sequence(&mut mutations)
            .with(eq(sudo(&[action, "--now", name]).unchecked()))
            .returning(move |_| {
                applied.store(enabled, Ordering::SeqCst);
                Ok(ExecResult::success(""))
            });
        let ctx = make_systemd_context(mock);
        assert!(
            task.needs_elevation(&ctx),
            "runtime drift needs system privileges"
        );
        let preview = task.run(&ctx.with_dry_run(true)).unwrap();
        assert!(matches!(preview, TaskResult::Batch(ref stats) if stats.changed_count() == 1));
        assert_eq!(
            active.load(Ordering::SeqCst),
            !enabled,
            "dry run changed runtime state"
        );
        let applied_result = task.run(&ctx).unwrap();
        assert!(
            matches!(applied_result, TaskResult::Batch(ref stats) if stats.changed_count() == 1)
        );
        assert_eq!(active.load(Ordering::SeqCst), enabled);
        assert!(
            !task.needs_elevation(&ctx),
            "converged service needs no elevation"
        );
        let repeated = task.run(&ctx).unwrap();
        assert!(
            matches!(repeated, TaskResult::Batch(ref stats) if stats.already_ok_count() == 1 && stats.changed_count() == 0)
        );
    }
}

#[test]
fn early_systemd_readiness_requires_converged_unit_state() {
    let name = "example.service";
    let enabled = || Ok(ExecResult::success("enabled\n"));
    let not_found = Ok(ExecResult::failure("not-found\n", "", Some(4)));
    let probe_error = Err(ExecError::spawn(
        "systemctl",
        std::io::Error::other("fixture failure"),
    ));
    for (enablement, runtime, ready) in [
        (enabled(), Some("active"), true),
        (enabled(), Some("inactive"), false),
        (not_found, None, false),
        (probe_error, None, false),
    ] {
        let mut executor = script().command(systemctl(&["is-enabled", name]), enablement);
        if let Some(active) = runtime {
            executor = executor.command(
                runtime_probe(&UnitScope::System, name),
                Ok(ExecResult::success(format!(
                    "ActiveState={active}\nType=simple\n"
                ))),
            );
        }
        let ctx = make_context(
            "/tmp".into(),
            None,
            Platform::new(Os::Linux, true),
            Arc::new(executor),
        );
        assert_eq!(
            crate::engine::requirement::ordering_requirements_satisfied(
                &ctx,
                &systemd_task(name, UnitScope::System, true).ordering_requirements(),
            ),
            ready,
            "runtime={runtime:?}"
        );
    }
}

#[test]
fn early_systemd_readiness_requires_the_command_and_a_usable_manager() {
    let offline = Ok(ExecResult::failure("offline", "", Some(1)));
    for ctx in [
        manager_context(Platform::new(Os::Linux, true), false, None),
        manager_context(
            Platform::new_wsl(),
            true,
            Some((systemctl(&["is-system-running"]), offline)),
        ),
    ] {
        assert!(
            !crate::engine::requirement::ordering_requirements_satisfied(
                &ctx,
                &systemd_task("example.service", UnitScope::System, true).ordering_requirements(),
            )
        );
    }
}

#[test]
fn failed_manager_probes_are_not_offline_or_inapplicable() {
    for (platform, probe) in [
        (Platform::new(Os::Linux, true), user_manager_probe()),
        (Platform::new_wsl(), systemctl(&["is-system-running"])),
    ] {
        for dry_run in [false, true] {
            let refused = ExecError::spawn(
                "systemctl",
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "manager probe refused",
                ),
            );
            let ctx = manager_context(platform, true, Some((probe.clone(), Err(refused))))
                .with_dry_run(dry_run);
            let error = systemd_task("example.service", UnitScope::User, true)
                .run(&ctx)
                .unwrap_err();
            assert!(error.downcast_ref::<ExecError>().is_some(), "{error:#}");
        }
    }
}
