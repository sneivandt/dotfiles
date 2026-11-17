use super::*;
use crate::domains::system::config::systemd_units::SystemdUnit;
use crate::engine::{Context, SkipKind, Task, TaskResult};
use crate::infra::ConfigHandle;
#[cfg(unix)]
use crate::infra::env::MapEnv;
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor, MockExecutor};
use crate::infra::platform::{Os, Platform};
use crate::test_helpers::{ContextBuilder, ScriptedExecutor, make_context, make_linux_context};
use std::sync::Arc;

fn disabled_result() -> ExecResult {
    ExecResult::failure("disabled\n", "", Some(1))
}

fn systemd_task(name: &str, scope: UnitScope, enabled: bool) -> ConfigureSystemd {
    ConfigureSystemd::new(ConfigHandle::new(vec![SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    }]))
}

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

// ------------------------------------------------------------------
// ConfigureSystemd::run
// ------------------------------------------------------------------

/// Build a context backed by `MockExecutor` for `run()` tests.
fn make_systemd_context(mut executor: MockExecutor) -> Context {
    executor
        .expect_which()
        .with(mockall::predicate::eq("systemctl"))
        .return_const(true);
    context(executor)
}

fn context(executor: impl Executor + 'static) -> Context {
    make_context(
        "/tmp".into(),
        None,
        Platform::new(Os::Linux, false),
        Arc::new(executor),
    )
}

fn systemctl(args: &[&str]) -> CommandSpec {
    CommandSpec::new("systemctl").args(args).unchecked()
}

fn runtime_probe(name: &str) -> CommandSpec {
    systemctl(&[
        "show",
        "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult",
        name,
    ])
}

fn script() -> ScriptedExecutor {
    ScriptedExecutor::new().with_programs(&["systemctl"])
}

fn graphical_probe() -> CommandSpec {
    CommandSpec::new("systemctl").args(&[
        "--user",
        "show",
        "--property=ActiveState",
        "--value",
        "graphical-session.target",
    ])
}

#[test]
fn graphical_units_enable_without_starting_until_the_session_is_active() {
    let name = "hypridle.service";
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![SystemdUnit {
        name: name.into(),
        scope: UnitScope::User,
        enabled: true,
        graphical: true,
    }]));
    for dry_run in [true, false] {
        let mut executor = script();
        for (session, enabled, running) in [
            ("inactive", false, false),
            ("inactive", true, false),
            ("activating", true, false),
            ("deactivating", true, false),
            ("failed", true, false),
            ("active", true, false),
            ("active", true, true),
        ] {
            executor = executor
                .command(
                    systemctl(&["--user", "show-environment"]),
                    Ok(ExecResult::success("")),
                )
                .command(graphical_probe(), Ok(ExecResult::success(session)));
            if !dry_run {
                executor = executor.command(
                    CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]),
                    Ok(ExecResult::success("")),
                );
            }
            executor = executor.command(
                systemctl(&["--user", "is-enabled", name]),
                Ok(if enabled {
                    ExecResult::success("enabled")
                } else {
                    disabled_result()
                }),
            );
            if session == "active" {
                executor = executor.command(
                    systemctl(&["--user", "show", "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult", name]),
                    Ok(ExecResult::success(if running { "ActiveState=active" } else { "ActiveState=inactive" })),
                );
            }
            if !dry_run && (!enabled || (session == "active" && !running)) {
                let args = if session == "active" {
                    vec!["--user", "enable", "--now", name]
                } else {
                    vec!["--user", "enable", name]
                };
                executor = executor.command(systemctl(&args), Ok(ExecResult::success("")));
            }
        }
        let ctx = context(executor).with_dry_run(dry_run);
        for changed in [1, 0, 0, 0, 0, 1, 0] {
            let result = task.run(&ctx).unwrap();
            assert_eq!(
                crate::test_helpers::task_batch(&result).changed_count(),
                changed
            );
            assert_eq!(
                crate::test_helpers::task_batch(&result).already_ok_count(),
                1 - changed
            );
        }
    }
}

#[test]
fn graphical_session_probe_errors_never_reload_or_start_units() {
    let units = vec![SystemdUnit {
        name: "hypridle.service".into(),
        scope: UnitScope::User,
        enabled: true,
        graphical: true,
    }];
    for response in [
        Ok(ExecResult::success("")),
        Ok(ExecResult::success("unexpected")),
        Err(ExecError::NonZero {
            command: "systemctl show".into(),
            result: ExecResult::failure("", "no bus", Some(1)),
        }),
    ] {
        let executor = script()
            .command(
                systemctl(&["--user", "show-environment"]),
                Ok(ExecResult::success("")),
            )
            .command(graphical_probe(), response);
        let task = ConfigureSystemd::new(ConfigHandle::new(units.clone()));
        assert!(task.run(&context(executor)).is_err());
    }
    let ctx = context(MockExecutor::new());
    assert!(!graphical_session_active(&ctx, &units, false).unwrap());
    let mut disabled = units;
    disabled[0].enabled = false;
    assert!(!graphical_session_active(&ctx, &disabled, true).unwrap());
}

#[test]
fn gtk_service_migration_runs_only_for_the_enabled_user_replacement() {
    let unit = |name: &str, scope, enabled| SystemdUnit {
        name: name.into(),
        scope,
        graphical: false,
        enabled,
    };
    let gtk = unit("gtk-settings.service", UnitScope::User, true);
    for (label, units, expected) in [
        ("replacement", vec![gtk.clone()], true),
        (
            "no replacement",
            vec![unit("other.service", UnitScope::User, true)],
            false,
        ),
        (
            "disabled replacement",
            vec![unit("gtk-settings.service", UnitScope::User, false)],
            false,
        ),
        (
            "system replacement",
            vec![unit("gtk-settings.service", UnitScope::System, true)],
            false,
        ),
        (
            "explicit user unit",
            vec![
                gtk.clone(),
                unit("nautilus-settings.service", UnitScope::User, true),
            ],
            false,
        ),
        (
            "unrelated system unit",
            vec![
                gtk,
                unit("nautilus-settings.service", UnitScope::System, true),
            ],
            true,
        ),
    ] {
        assert_eq!(should_retire_nautilus_settings(&units), expected, "{label}");
    }
}

#[cfg(unix)]
#[test]
fn gtk_service_migration_previews_then_removes_retired_links_once() {
    let task = systemd_task("gtk-settings.service", UnitScope::User, true);
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("checkout");
    let home = fixture.path().join("home");
    let source_dir = root.join("symlinks/config/systemd/user");
    let user_dir = home.join(".config/systemd/user");
    let wants = user_dir.join("graphical-session.target.wants");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&wants).unwrap();
    let old_name = "nautilus-settings.service";
    let old_source = source_dir.join(old_name);
    let old_unit = user_dir.join(old_name);
    let old_enablement = wants.join(old_name);
    std::os::unix::fs::symlink(&old_source, &old_unit).unwrap();
    std::os::unix::fs::symlink(&old_source, &old_enablement).unwrap();
    let gtk = user_dir.join("gtk-settings.service");
    std::fs::write(&gtk, "[Install]\nWantedBy=graphical-session.target\n").unwrap();
    std::os::unix::fs::symlink(&gtk, wants.join("gtk-settings.service")).unwrap();

    let mut executor = script();
    for _ in 0..3 {
        executor = executor.command(
            systemctl(&["--user", "show-environment"]),
            Ok(ExecResult::failure("", "No user manager", Some(1))),
        );
    }
    let ctx = make_context(
        root,
        None,
        Platform::new(Os::Linux, true),
        Arc::new(executor),
    )
    .with_home(home);
    let preview = task.run(&ctx.with_dry_run(true)).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&preview).changed_count(), 1);
    assert_eq!(std::fs::read_link(&old_unit).unwrap(), old_source);
    assert_eq!(std::fs::read_link(&old_enablement).unwrap(), old_source);
    let applied = task.run(&ctx).unwrap();
    assert_eq!(crate::test_helpers::task_batch(&applied).changed_count(), 1);
    assert!(!old_unit.is_symlink());
    assert!(!old_enablement.is_symlink());
    let repeated = task.run(&ctx).unwrap();
    assert_eq!(
        crate::test_helpers::task_batch(&repeated).changed_count(),
        0
    );
}

#[test]
fn run_calls_daemon_reload_before_enabling_unit() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let task = systemd_task("dunst.service", UnitScope::User, true);
    // Ordered expectations:
    //   1. probe the live user manager
    //   2. run("systemctl", ["--user", "daemon-reload"]) -> success
    //   3. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) -> disabled
    //   4. run_unchecked("systemctl", ["--user", "enable", "--now", "dunst.service"]) → success
    let mut seq = mockall::Sequence::new();
    let mut mock = MockExecutor::new();
    let installed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&installed);
    mock.expect_which()
        .once()
        .with(mockall::predicate::eq("systemctl"))
        .returning(move |_| observed.load(Ordering::SeqCst));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["--user", "show-environment"]
                && !spec.is_checked()
                && spec.working_dir().is_none()
        })
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["--user", "daemon-reload"]
                && spec.is_checked()
                && spec.working_dir().is_none()
        })
        .returning(|_| Ok(ExecResult::success("")));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["--user", "is-enabled", "dunst.service"]
                && !spec.is_checked()
                && spec.working_dir().is_none()
        })
        .returning(|_| Ok(disabled_result()));
    mock.expect_execute()
        .once()
        .in_sequence(&mut seq)
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["--user", "enable", "--now", "dunst.service"]
                && !spec.is_checked()
                && spec.working_dir().is_none()
        })
        .returning(|_| Ok(ExecResult::success("")));
    let ctx = make_systemd_context(mock);
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
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .once()
        .with(mockall::predicate::eq("systemctl"))
        .return_const(true);
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["is-system-running"]
                && !spec.is_checked()
        })
        .returning(|_| Ok(ExecResult::failure("offline", "", Some(1))));
    let ctx = make_context(
        ".".into(),
        None,
        Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: true,
        },
        Arc::new(mock),
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
            Platform {
                os: Os::Linux,
                is_arch: false,
                is_wsl: true,
            },
            Arc::new(executor),
        );
        assert_eq!(systemd_available(&ctx).unwrap(), expected, "{name}");
    }
}

#[test]
fn run_skips_daemon_reload_in_dry_run() {
    let task = systemd_task("dunst.service", UnitScope::User, true);
    let executor = script()
        .command(
            systemctl(&["--user", "show-environment"]),
            Ok(ExecResult::success("")),
        )
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
            systemctl(&["--user", "show-environment"]),
            ExecResult::success(""),
            CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]),
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
fn run_enables_user_units_offline_when_the_user_manager_is_unavailable() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("clean-home-tmp.timer"),
        "[Install]\nWantedBy=timers.target\n",
    )
    .unwrap();
    let task = systemd_task("clean-home-tmp.timer", UnitScope::User, true);
    let mut mock = MockExecutor::new();
    mock.expect_execute()
        .once()
        .withf(|spec| {
            spec.program() == "systemctl"
                && spec.arguments() == ["--user", "show-environment"]
                && !spec.is_checked()
        })
        .returning(|_| {
            Ok(ExecResult::failure(
                "",
                "Failed to connect to bus: No medium found",
                Some(1),
            ))
        });
    let ctx = make_systemd_context(mock).with_home(home.path().to_path_buf());

    let result = task.run(&ctx).unwrap();

    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
        "offline enablement should be reported as changed: {result:?}"
    );
    assert!(
        unit_dir
            .join("timers.target.wants/clean-home-tmp.timer")
            .is_symlink(),
        "offline enablement should converge before the first login"
    );
}

#[cfg(unix)]
#[test]
fn arch_chroot_provisioning_never_probes_the_user_manager() {
    let home = tempfile::tempdir().unwrap();
    let unit_dir = home.path().join(".config/systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("clean-home-tmp.timer"),
        "[Install]\nWantedBy=timers.target\n",
    )
    .unwrap();
    let task = systemd_task("clean-home-tmp.timer", UnitScope::User, true);
    let ctx = make_systemd_context(MockExecutor::new())
        .with_home(home.path().to_path_buf())
        .with_env(
            MapEnv::new()
                .with(
                    crate::infra::provisioning::ENV_VAR,
                    crate::infra::provisioning::ARCH_CHROOT,
                )
                .into_handle(),
        );

    let result = task.run(&ctx).unwrap();

    assert!(
        matches!(result, TaskResult::Batch(ref stats) if stats.changed_count() == 1),
        "offline provisioning should report one changed unit: {result:?}"
    );
    assert!(
        unit_dir
            .join("timers.target.wants/clean-home-tmp.timer")
            .is_symlink(),
        "offline provisioning should enable the timer without using the user bus"
    );
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
                runtime_probe(name),
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
                runtime_probe(name),
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
            runtime_probe(name),
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
    let mut executor = executor.command(
        systemctl(&["--user", "show-environment"]),
        Ok(ExecResult::success("")),
    );
    if !dry_run {
        executor = executor.command(
            CommandSpec::new("systemctl").args(&["--user", "daemon-reload"]),
            Ok(ExecResult::success("")),
        );
    }
    executor
        .command(
            systemctl(&["--user", "is-enabled", name]),
            Ok(ExecResult::success("enabled\n")),
        )
        .command(
            systemctl(&[
                "--user",
                "show",
                "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult",
                name,
            ]),
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
fn runtime_drift_plans_elevation_respects_dry_run_and_converges() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for enabled in [true, false] {
        let task = systemd_task("test.service", UnitScope::System, enabled);
        let active = Arc::new(AtomicBool::new(!enabled));
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .withf(|spec| {
                spec.program() == "systemctl"
                    && spec.arguments() == ["is-enabled", "test.service"]
                    && !spec.is_checked()
            })
            .returning(move |_| {
                Ok(if enabled {
                    ExecResult::success("enabled\n")
                } else {
                    disabled_result()
                })
            });
        let observed = Arc::clone(&active);
        mock.expect_execute()
                .withf(|spec| {
                    spec.program() == "systemctl"
                        && spec.arguments() == [
                            "show",
                            "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult",
                            "test.service",
                        ]
                        && !spec.is_checked()
                })
                .returning(move |_| {
                    Ok(ExecResult::success(if observed.load(Ordering::SeqCst) {
                        "ActiveState=active\n"
                    } else {
                        "ActiveState=inactive\n"
                    }))
                });
        mock.expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "sudo"
                    && spec.arguments() == ["systemctl", "daemon-reload"]
                    && spec.is_checked()
            })
            .returning(|_| Ok(ExecResult::success("")));
        let applied = Arc::clone(&active);
        if !enabled {
            mock.expect_execute()
                .once()
                .withf(|spec| {
                    spec.program() == "sudo"
                        && spec.arguments() == ["systemctl", "disable", "--runtime", "test.service"]
                        && !spec.is_checked()
                })
                .returning(|_| Ok(ExecResult::success("")));
        }
        mock.expect_execute()
            .once()
            .withf(move |spec| {
                spec.program() == "sudo"
                    && spec.arguments()
                        == [
                            "systemctl",
                            if enabled { "enable" } else { "disable" },
                            "--now",
                            "test.service",
                        ]
                    && !spec.is_checked()
            })
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
    for (case, expected) in [
        ("active", true),
        ("inactive", false),
        ("missing", false),
        ("probe-error", false),
    ] {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .withf(|program| program == "systemctl")
            .return_const(true);
        mock.expect_execute()
            .once()
            .withf(|spec| spec.arguments() == ["is-enabled", "example.service"])
            .returning(move |_| {
                if case == "probe-error" {
                    return Err(ExecError::spawn(
                        "systemctl",
                        std::io::Error::other("fixture failure"),
                    ));
                }
                Ok(if case == "missing" {
                    ExecResult::failure("not-found\n", "", Some(4))
                } else {
                    ExecResult::success("enabled\n")
                })
            });
        if matches!(case, "active" | "inactive") {
            mock.expect_execute()
                .once()
                .withf(|spec| spec.arguments().first().is_some_and(|arg| arg == "show"))
                .returning(move |_| {
                    Ok(ExecResult::success(format!(
                        "ActiveState={case}\nType=simple\n"
                    )))
                });
        }
        let ctx = make_context(
            "/tmp".into(),
            None,
            Platform::new(Os::Linux, true),
            Arc::new(mock),
        );
        assert_eq!(
            crate::engine::requirement::ordering_requirements_satisfied(
                &ctx,
                &systemd_task("example.service", UnitScope::System, true).ordering_requirements(),
            ),
            expected,
            "{case}"
        );
    }
}

#[test]
fn early_systemd_readiness_requires_the_command_and_a_usable_manager() {
    for wsl in [false, true] {
        let mut mock = MockExecutor::new();
        mock.expect_which()
            .once()
            .withf(|program| program == "systemctl")
            .return_const(wsl);
        if wsl {
            mock.expect_execute()
                .once()
                .withf(|spec| {
                    spec.program() == "systemctl" && spec.arguments() == ["is-system-running"]
                })
                .returning(|_| Ok(ExecResult::failure("offline", "", Some(1))));
        }

        let platform = if wsl {
            Platform::new_wsl()
        } else {
            Platform::new(Os::Linux, true)
        };
        let ctx = make_context("/tmp".into(), None, platform, Arc::new(mock));
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
    for wsl in [false, true] {
        for dry_run in [false, true] {
            let mut mock = MockExecutor::new();
            mock.expect_which()
                .once()
                .with(mockall::predicate::eq("systemctl"))
                .return_const(true);
            mock.expect_execute()
                .once()
                .withf(move |spec| {
                    spec.program() == "systemctl"
                        && !spec.is_checked()
                        && spec.arguments()
                            == if wsl {
                                vec!["is-system-running"]
                            } else {
                                vec!["--user", "show-environment"]
                            }
                })
                .returning(|_| {
                    Err(ExecError::spawn(
                        "systemctl",
                        std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "manager probe refused",
                        ),
                    ))
                });
            let ctx = make_context(
                "/tmp".into(),
                None,
                if wsl {
                    Platform::new_wsl()
                } else {
                    Platform::new(Os::Linux, true)
                },
                Arc::new(mock),
            )
            .with_dry_run(dry_run);
            let error = systemd_task("example.service", UnitScope::User, true)
                .run(&ctx)
                .unwrap_err();
            assert!(error.downcast_ref::<ExecError>().is_some(), "{error:#}");
        }
    }
}
