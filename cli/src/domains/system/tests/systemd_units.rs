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
fn probe_system_reload_state_distinguishes_cancellation_and_wsl_rejects_unknown_exit_zero() {
    let cancel_exec = Arc::new({
        let mut mock = MockExecutor::new();
        mock.expect_execute().returning(|spec| {
            Err(ExecError::Cancelled {
                command: spec.program().to_string_lossy().into_owned(),
                result: ExecResult::failure("", "", None),
            })
        });
        mock
    });
    let unit = SystemdUnit {
        name: "sshd.service".into(),
        scope: UnitScope::System,
        graphical: false,
        enabled: true,
    };
    let cancel_clone = Arc::clone(&cancel_exec);
    let cancel_exec_handle: Arc<dyn Executor> = cancel_clone;
    let resources = vec![SystemdUnitResource::from_entry(
        &unit,
        cancel_exec_handle,
        std::path::Path::new("/home/test"),
        true,
        false,
    )];
    let cancel_err = probe_system_reload_state(cancel_exec.as_ref(), &resources, true).unwrap_err();
    assert!(cancel_err.is_cancelled());

    let fail_exec = Arc::new({
        let mut mock = MockExecutor::new();
        mock.expect_execute().returning(|spec| {
            Err(ExecError::Spawn {
                command: spec.program().to_string_lossy().into_owned(),
                source: std::io::Error::other("boom"),
            })
        });
        mock
    });
    let fail_clone = Arc::clone(&fail_exec);
    let fail_exec_handle: Arc<dyn Executor> = fail_clone;
    let fail_resources = vec![SystemdUnitResource::from_entry(
        &unit,
        fail_exec_handle,
        std::path::Path::new("/home/test"),
        true,
        false,
    )];
    assert_eq!(
        probe_system_reload_state(fail_exec.as_ref(), &fail_resources, true).unwrap(),
        (false, false)
    );

    let mut wsl_mock = MockExecutor::new();
    wsl_mock
        .expect_execute()
        .once()
        .returning(|_| Ok(ExecResult::success("weird-state\n")));
    let wsl_ctx = make_context(
        "/fixture".into(),
        None,
        Platform::new_wsl(),
        Arc::new(wsl_mock),
    );
    let wsl_err = systemd_available(&wsl_ctx).unwrap_err();
    assert!(
        wsl_err
            .to_string()
            .contains("unexpected systemctl is-system-running state"),
        "{wsl_err:#}"
    );
}

#[path = "systemd_units_managers.rs"]
mod managers;
#[path = "systemd_units_runtime.rs"]
mod runtime;
#[path = "systemd_units_session.rs"]
mod session;
