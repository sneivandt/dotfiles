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
fn current_system_unit_is_probed_once_in_preview_and_apply() {
    let name = "NetworkManager.service";
    let task = systemd_task(name, UnitScope::System, true);
    for dry_run in [false, true] {
        let executor = script()
            .command(
                systemctl(&["is-enabled", name]),
                Ok(ExecResult::success("enabled\n")),
            )
            .command(
                runtime_probe(&UnitScope::System, name),
                Ok(ExecResult::success("ActiveState=active\n")),
            );
        let result = task.run(&context(executor).with_dry_run(dry_run)).unwrap();
        assert_eq!(
            crate::test_helpers::task_batch(&result).already_ok_count(),
            1
        );
    }
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
fn a_system_reload_invalidates_other_units_previously_observed_as_current() {
    use crate::domains::system::resources::systemd_unit::BATCH_SHOW_PROPERTIES;

    let first = "first.timer";
    let stale = "stale.timer";
    let task = ConfigureSystemd::new(ConfigHandle::new(
        [first, stale]
            .map(|name| SystemdUnit {
                name: name.into(),
                scope: UnitScope::System,
                enabled: true,
                graphical: false,
            })
            .to_vec(),
    ));
    let prop_arg = format!("--property={BATCH_SHOW_PROPERTIES}");
    let batch_enabled = || systemctl(&["is-enabled", first, stale]);
    let batch_show = || systemctl(&["show", &prop_arg, first, stale]);
    let pre_reload_stdout = format!(
        "Id={first}\nLoadState=loaded\nActiveState=active\nNeedDaemonReload=no\n\n\
         Id={stale}\nLoadState=loaded\nActiveState=active\nNeedDaemonReload=yes\n"
    );
    let post_reload_stdout = format!(
        "Id={first}\nLoadState=loaded\nActiveState=inactive\nNeedDaemonReload=no\n\n\
         Id={stale}\nLoadState=loaded\nActiveState=active\nNeedDaemonReload=no\n"
    );
    let executor = script()
        .command(
            batch_enabled(),
            Ok(ExecResult::success("enabled\nenabled\n")),
        )
        .command(batch_show(), Ok(ExecResult::success(pre_reload_stdout)))
        .command(
            CommandSpec::new("sudo").args(&["systemctl", "daemon-reload"]),
            Ok(ExecResult::success("")),
        )
        .command(
            batch_enabled(),
            Ok(ExecResult::success("disabled\nenabled\n")),
        )
        .command(batch_show(), Ok(ExecResult::success(post_reload_stdout)))
        .command(
            CommandSpec::new("sudo")
                .args(&["systemctl", "enable", "--now", first])
                .unchecked(),
            Ok(ExecResult::success("")),
        )
        // Mutating `first` invalidates the remaining cached scope entries so `stale` re-probes live.
        .command(
            systemctl(&["is-enabled", stale]),
            Ok(ExecResult::success("enabled")),
        )
        .command(
            runtime_probe(&UnitScope::System, stale),
            Ok(ExecResult::success("ActiveState=active\n")),
        );
    let result = task.run(&context(executor)).unwrap();
    let result = crate::test_helpers::task_batch(&result);
    assert_eq!(result.changed_count(), 1);
    assert_eq!(result.already_ok_count(), 1);
}

#[test]
fn batch_probe_converges_multiple_steady_state_units_in_one_show_call() {
    use crate::domains::system::resources::systemd_unit::BATCH_SHOW_PROPERTIES;

    let units = ["alpha.service", "beta.timer", "gamma.service"];
    let task = ConfigureSystemd::new(ConfigHandle::new(
        units
            .iter()
            .map(|name| SystemdUnit {
                name: (*name).into(),
                scope: UnitScope::User,
                enabled: true,
                graphical: false,
            })
            .collect(),
    ));
    let prop_arg = format!("--property={BATCH_SHOW_PROPERTIES}");
    let batch_stdout = units
        .iter()
        .map(|name| format!("Id={name}\nLoadState=loaded\nActiveState=active\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let executor = script()
        .command(user_manager_probe(), Ok(ExecResult::success("")))
        .command(user_reload(), Ok(ExecResult::success("")))
        .command(
            systemctl(&["--user", "is-enabled", units[0], units[1], units[2]]),
            Ok(ExecResult::success("enabled\nenabled\nenabled\n")),
        )
        .command(
            systemctl(&["--user", "show", &prop_arg, units[0], units[1], units[2]]),
            Ok(ExecResult::success(batch_stdout)),
        );
    let result = task.run(&context(executor)).unwrap();
    let stats = crate::test_helpers::task_batch(&result);
    assert_eq!(stats.already_ok_count(), 3);
    assert_eq!(stats.changed_count(), 0);
}

#[test]
fn batch_probe_falls_back_to_is_enabled_for_ambiguous_unit_file_states() {
    use crate::domains::system::resources::systemd_unit::BATCH_SHOW_PROPERTIES;

    let alias = "alias.service";
    let runtime_only = "runtime.service";
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![
        SystemdUnit {
            name: alias.into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
        SystemdUnit {
            name: runtime_only.into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
    ]));
    let prop_arg = format!("--property={BATCH_SHOW_PROPERTIES}");
    let batch_stdout = format!(
        "Id=canonical.service\nLoadState=loaded\nActiveState=active\n\n\
         Id={runtime_only}\nLoadState=loaded\nActiveState=active\n"
    );
    let executor = script()
        .command(
            systemctl(&["is-enabled", alias, runtime_only]),
            Ok(ExecResult::success("alias\nenabled-runtime\n")),
        )
        .command(
            systemctl(&["show", &prop_arg, alias, runtime_only]),
            Ok(ExecResult::success(batch_stdout)),
        )
        // Alias falls back to individual is-enabled -> alias_state().
        .command(
            systemctl(&["is-enabled", alias]),
            Ok(ExecResult::success("alias\n")),
        )
        .command(
            systemctl(&["show", "--property=Id", "--value", alias]),
            Ok(ExecResult::success("canonical.service\n")),
        )
        // enabled-runtime falls back to individual is-enabled -> Missing.
        .command(
            systemctl(&["is-enabled", runtime_only]),
            Ok(ExecResult::success("enabled-runtime\n")),
        );
    let result = task.run(&context(executor).with_dry_run(true)).unwrap();
    let stats = crate::test_helpers::task_batch(&result);
    assert_eq!(stats.failed_count(), 1, "alias unit must report Invalid");
    assert_eq!(
        stats.changed_count(),
        1,
        "enabled-runtime unit must plan persistent enablement"
    );
}

#[test]
fn batch_probe_uses_authoritative_is_enabled_over_stale_show_unit_file_state() {
    use crate::domains::system::resources::systemd_unit::BATCH_SHOW_PROPERTIES;

    assert!(
        !BATCH_SHOW_PROPERTIES.contains("UnitFileState"),
        "batch show must not rely on cached manager UnitFileState"
    );

    let unlinked = "unlinked.service";
    let intact = "intact.service";
    let task = ConfigureSystemd::new(ConfigHandle::new(
        [unlinked, intact]
            .map(|name| SystemdUnit {
                name: name.into(),
                scope: UnitScope::System,
                enabled: true,
                graphical: false,
            })
            .to_vec(),
    ));
    let prop_arg = format!("--property={BATCH_SHOW_PROPERTIES}");
    // Even if `systemctl show` emitted a stale cached UnitFileState=enabled for an
    // active unit whose .wants/ symlink was removed on disk, authoritative
    // `is-enabled` reports disabled so the unit does not falsely converge.
    let stale_show_stdout = format!(
        "Id={unlinked}\nLoadState=loaded\nUnitFileState=enabled\nActiveState=active\nNeedDaemonReload=no\n\n\
         Id={intact}\nLoadState=loaded\nUnitFileState=enabled\nActiveState=active\nNeedDaemonReload=no\n"
    );
    let executor = script()
        .command(
            systemctl(&["is-enabled", unlinked, intact]),
            Ok(ExecResult::success("disabled\nenabled\n")),
        )
        .command(
            systemctl(&["show", &prop_arg, unlinked, intact]),
            Ok(ExecResult::success(stale_show_stdout)),
        );
    let result = task.run(&context(executor).with_dry_run(true)).unwrap();
    let stats = crate::test_helpers::task_batch(&result);
    assert_eq!(
        stats.changed_count(),
        1,
        "unit whose on-disk link was removed must plan enablement"
    );
    assert_eq!(stats.already_ok_count(), 1);
}

#[test]
fn user_batch_probe_timeout_falls_back_without_aborting_system_units() {
    use crate::domains::system::resources::systemd_unit::BATCH_SHOW_PROPERTIES;

    let usr_a = "usr-a.service";
    let usr_b = "usr-b.service";
    let sys_a = "sys-a.service";
    let sys_b = "sys-b.service";
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![
        SystemdUnit {
            name: usr_a.into(),
            scope: UnitScope::User,
            enabled: true,
            graphical: false,
        },
        SystemdUnit {
            name: usr_b.into(),
            scope: UnitScope::User,
            enabled: true,
            graphical: false,
        },
        SystemdUnit {
            name: sys_a.into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
        SystemdUnit {
            name: sys_b.into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
    ]));
    let prop_arg = format!("--property={BATCH_SHOW_PROPERTIES}");
    let system_show_stdout = format!(
        "Id={sys_a}\nLoadState=loaded\nActiveState=active\nNeedDaemonReload=no\n\n\
         Id={sys_b}\nLoadState=loaded\nActiveState=active\nNeedDaemonReload=no\n"
    );
    let timeout = || ExecError::TimedOut {
        command: "systemctl".into(),
        timeout: std::time::Duration::from_secs(30),
        result: ExecResult::failure("", "", None),
    };
    let executor = script()
        .command(user_manager_probe(), Ok(ExecResult::success("")))
        .command(
            systemctl(&["is-enabled", sys_a, sys_b]),
            Ok(ExecResult::success("enabled\nenabled\n")),
        )
        .command(
            systemctl(&["show", &prop_arg, sys_a, sys_b]),
            Ok(ExecResult::success(system_show_stdout)),
        )
        .command(user_reload(), Ok(ExecResult::success("")))
        // User-scope batch probe times out; system units are unaffected and user
        // units fall back to independent per-unit discovery under lenient processing.
        .command(
            systemctl(&["--user", "is-enabled", usr_a, usr_b]),
            Err(timeout()),
        )
        .command(
            systemctl(&["--user", "is-enabled", usr_a]),
            Ok(ExecResult::failure("", "unit query failed", Some(1))),
        )
        .command(
            systemctl(&["--user", "is-enabled", usr_b]),
            Ok(ExecResult::success("enabled\n")),
        )
        .command(
            runtime_probe(&UnitScope::User, usr_b),
            Ok(ExecResult::success("ActiveState=active\n")),
        );
    let result = task.run(&context(executor)).unwrap();
    let stats = crate::test_helpers::task_batch(&result);
    assert_eq!(
        stats.already_ok_count(),
        3,
        "both system units and healthy user unit must converge"
    );
    assert_eq!(
        stats.failed_count(),
        1,
        "only the failing user unit should be recorded as failed"
    );
}

#[test]
fn batch_probe_cancellation_propagates_without_fallback() {
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![
        SystemdUnit {
            name: "alpha.service".into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
        SystemdUnit {
            name: "beta.service".into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
    ]));
    let executor = script().command(
        systemctl(&["is-enabled", "alpha.service", "beta.service"]),
        Err(ExecError::Cancelled {
            command: "systemctl".into(),
            result: ExecResult::failure("", "", None),
        }),
    );
    let err = task
        .run(&context(executor).with_dry_run(true))
        .expect_err("cancellation must abort instead of falling back");
    assert!(
        err.downcast_ref::<crate::engine::resource::ResourceError>()
            .is_some_and(crate::engine::resource::ResourceError::is_cancelled),
        "unexpected error: {err:#}"
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
fn early_systemd_readiness_rejects_invalid_unit_scope() {
    use crate::engine::requirement::RequirementStage;

    let valid = "example.service";
    let invalid = "bad.service";
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![
        SystemdUnit {
            name: valid.into(),
            scope: UnitScope::System,
            enabled: true,
            graphical: false,
        },
        SystemdUnit {
            name: invalid.into(),
            scope: UnitScope::Invalid("bogus".into()),
            enabled: true,
            graphical: false,
        },
    ]));
    let executor = script()
        .command(
            systemctl(&["is-enabled", valid]),
            Ok(ExecResult::success("enabled\n")),
        )
        .command(
            runtime_probe(&UnitScope::System, valid),
            Ok(ExecResult::success("ActiveState=active\nType=simple\n")),
        );
    let ctx = make_context(
        "/tmp".into(),
        None,
        Platform::new(Os::Linux, true),
        Arc::new(executor),
    );
    let requirements = task.ordering_requirements();
    let units_req = requirements
        .iter()
        .find(|req| req.id() == "systemd-units-current")
        .expect("systemd-units-current requirement");
    match units_req.check(&ctx, RequirementStage::Ordering) {
        RequirementState::Unsatisfied { reason } => {
            assert!(
                reason.contains(invalid) && reason.contains("bogus"),
                "expected invalid unit and scope in reason, got: {reason}"
            );
        }
        other @ (RequirementState::Satisfied(()) | RequirementState::Unknown { .. }) => {
            panic!("expected Unsatisfied for invalid unit scope, got: {other:?}")
        }
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
