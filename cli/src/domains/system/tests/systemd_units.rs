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
            .command(systemctl(&["is-enabled", name]), Ok(observed))
            .command(
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
