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

#[path = "systemd_units_managers.rs"]
mod managers;
#[path = "systemd_units_runtime.rs"]
mod runtime;
#[path = "systemd_units_session.rs"]
mod session;
