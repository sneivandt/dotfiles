//! Expected `systemctl` probes shared by task and resource tests.

use super::config::systemd_units::UnitScope;
use crate::infra::exec::CommandSpec;

pub(super) fn systemctl(args: &[&str]) -> CommandSpec {
    CommandSpec::new("systemctl").args(args).unchecked()
}

pub(super) fn unit_probe(scope: &UnitScope, name: &str, args: &[&str]) -> CommandSpec {
    let prefix = match scope {
        UnitScope::User => &["--user"][..],
        UnitScope::System => &[],
        UnitScope::Invalid(_) => panic!("invalid scope must not be queried"),
    };
    systemctl(prefix).args(args).arg(name)
}

pub(super) fn runtime_probe(scope: &UnitScope, name: &str) -> CommandSpec {
    unit_probe(
        scope,
        name,
        &[
            "show",
            "--property=ActiveState,Type,Result,ExecMainStartTimestampMonotonic,ConditionResult,NeedDaemonReload",
        ],
    )
}
