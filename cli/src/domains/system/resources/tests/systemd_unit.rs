use std::sync::Arc;

use super::*;
use crate::domains::system::systemd_fixture::{runtime_probe, systemctl, unit_probe};
use crate::infra::exec::{ExecResult, MockExecutor};
use crate::test_helpers::ScriptedExecutor;

#[cfg(unix)]
fn offline_unit(home: &Path, name: &str, enabled: bool) -> SystemdUnitResource {
    let entry = crate::domains::system::config::systemd_units::SystemdUnit {
        name: name.into(),
        scope: UnitScope::User,
        graphical: false,
        enabled,
    };
    SystemdUnitResource::from_entry(&entry, Arc::new(MockExecutor::new()), home, false, false)
}

fn queried_unit(
    name: &str,
    scope: UnitScope,
    enabled: bool,
    enablement: ExecResult,
    runtime: Option<&str>,
) -> SystemdUnitResource {
    let mut script =
        ScriptedExecutor::new().command(unit_probe(&scope, name, &["is-enabled"]), Ok(enablement));
    if let Some(runtime) = runtime {
        script = script.command(
            runtime_probe(&scope, name),
            Ok(ExecResult::success(runtime)),
        );
    }
    let mut resource = SystemdUnitResource::new(name, scope, Arc::new(script));
    resource.enabled = enabled;
    resource
}

#[path = "systemd_unit_install_targets.rs"]
mod install_targets;
#[path = "systemd_unit_offline.rs"]
mod offline;
#[path = "systemd_unit_runtime.rs"]
mod runtime;
