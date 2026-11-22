use super::*;
use crate::domains::system::config::systemd_units::SystemdUnit;
use crate::domains::system::systemd_fixture::{runtime_probe, systemctl};
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

fn user_manager_probe() -> CommandSpec {
    systemctl(&["--user", "show-environment"])
        .env("LC_ALL", "C")
        .output_log(OutputLog::Omit)
}

fn user_reload() -> CommandSpec {
    CommandSpec::new("systemctl").args(&["--user", "daemon-reload"])
}

fn script() -> ScriptedExecutor {
    ScriptedExecutor::new().with_programs(&["systemctl"])
}

fn graphical_probe(
    script: ScriptedExecutor,
    result: Result<ExecResult, ExecError>,
) -> ScriptedExecutor {
    let probe = CommandSpec::new("systemctl").args(&[
        "--user",
        "show",
        "--property=ActiveState",
        "--value",
        "graphical-session.target",
    ]);
    script
        .command(user_manager_probe(), Ok(ExecResult::success("")))
        .command(probe, result)
}

fn graphical_unit() -> SystemdUnit {
    SystemdUnit {
        name: "hypridle.service".into(),
        scope: UnitScope::User,
        enabled: true,
        graphical: true,
    }
}

#[test]
fn graphical_units_enable_without_starting_until_the_session_is_active() {
    use UnitProbe::{Disabled, Enabled, Runtime};

    #[derive(Debug, Clone, Copy)]
    enum UnitProbe {
        Disabled,
        Enabled,
        Runtime(&'static str),
    }

    let unit = "hypridle.service";
    let enable = ["--user", "enable", unit];
    let start = ["--user", "enable", "--now", unit];
    let cases = [
        ("inactive", Disabled, Some(enable.as_slice())),
        ("inactive", Enabled, None),
        ("activating", Enabled, None),
        ("deactivating", Enabled, None),
        ("failed", Enabled, None),
        (
            "active",
            Runtime("ActiveState=inactive"),
            Some(start.as_slice()),
        ),
        ("active", Runtime("ActiveState=active"), None),
    ];
    let task = ConfigureSystemd::new(ConfigHandle::new(vec![graphical_unit()]));
    for dry_run in [true, false] {
        let mut executor = script();
        for (session, observed, action) in cases {
            executor = graphical_probe(executor, Ok(ExecResult::success(session)));
            if !dry_run {
                executor = executor.command(user_reload(), Ok(ExecResult::success("")));
            }
            let enablement = match observed {
                Disabled => disabled_result(),
                Enabled | Runtime(_) => ExecResult::success("enabled"),
            };
            executor = executor.command(systemctl(&["--user", "is-enabled", unit]), Ok(enablement));
            if let Runtime(properties) = observed {
                executor = executor.command(
                    runtime_probe(&UnitScope::User, unit),
                    Ok(ExecResult::success(properties)),
                );
            }
            if !dry_run && let Some(args) = action {
                executor = executor.command(systemctl(args), Ok(ExecResult::success("")));
            }
        }
        let ctx = context(executor).with_dry_run(dry_run);
        for (session, observed, action) in cases {
            let result = task.run(&ctx).unwrap();
            let stats = crate::test_helpers::task_batch(&result);
            let expected = if action.is_some() { (1, 0) } else { (0, 1) };
            let counts = (stats.changed_count(), stats.already_ok_count());
            assert_eq!(
                counts, expected,
                "{session}, {observed:?}, dry_run={dry_run}"
            );
            assert_eq!(
                (stats.failed_count(), stats.skipped_count()),
                (0, 0),
                "{result:?}"
            );
        }
    }
}

#[test]
fn graphical_session_probe_errors_never_reload_or_start_units() {
    let units = vec![graphical_unit()];
    let task = ConfigureSystemd::new(ConfigHandle::new(units.clone()));
    for dry_run in [true, false] {
        for (response, diagnostic) in [
            (
                Ok(ExecResult::success("")),
                "unexpected graphical-session.target ActiveState: \"\"",
            ),
            (
                Ok(ExecResult::success("unexpected")),
                "unexpected graphical-session.target ActiveState: \"unexpected\"",
            ),
            (
                Err(ExecError::NonZero {
                    command: "systemctl show".into(),
                    result: ExecResult::failure("", "no bus", Some(1)),
                }),
                "no bus",
            ),
        ] {
            let executor = graphical_probe(script(), response);
            let error = task
                .run(&context(executor).with_dry_run(dry_run))
                .unwrap_err();
            assert!(format!("{error:#}").contains(diagnostic), "{error:#}");
        }
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
            user_manager_probe(),
            Ok(ExecResult::failure(
                "",
                "Failed to connect to bus: No medium found",
                Some(1),
            )),
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

/// These early-exit paths perform one availability lookup and at most one probe.
fn manager_context(
    platform: Platform,
    available: bool,
    probe: Option<(CommandSpec, Result<ExecResult, ExecError>)>,
) -> Context {
    let mut mock = MockExecutor::new();
    mock.expect_which()
        .once()
        .with(mockall::predicate::eq("systemctl"))
        .return_const(available);
    if let Some((cmd, result)) = probe {
        mock.expect_execute()
            .once()
            .with(mockall::predicate::eq(cmd))
            .return_once(|_| result);
    }
    make_context("/fixture".into(), None, platform, Arc::new(mock))
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
