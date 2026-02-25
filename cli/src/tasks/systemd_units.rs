//! Task: configure systemd user units.
use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources, task_deps};
use crate::resources::systemd_unit::SystemdUnitResource;

/// Enable and start systemd user units.
#[derive(Debug)]
pub struct ConfigureSystemd;

impl Task for ConfigureSystemd {
    fn name(&self) -> &'static str {
        "Configure systemd units"
    }

    task_deps![super::symlinks::InstallSymlinks];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_systemd()
            && !ctx.config_read().units.is_empty()
            && ctx.executor.which("systemctl")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Reload systemd daemon once before processing (idempotent and fast)
        if !ctx.dry_run
            && let Err(e) = ctx.executor.run("systemctl", &["--user", "daemon-reload"])
        {
            ctx.log.debug(&format!("daemon-reload failed: {e}"));
        }

        let units: Vec<_> = ctx.config_read().units.clone();
        let resources = units
            .iter()
            .map(|entry| SystemdUnitResource::from_entry(entry, &*ctx.executor));
        process_resources(ctx, resources, &ProcessOpts::install_missing("enable"))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::systemd_units::SystemdUnit;
    use crate::platform::Os;
    use crate::tasks::test_helpers::{
        empty_config, make_linux_context, make_platform_context_with_which,
    };
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
        });
        let ctx = make_platform_context_with_which(config, Os::Windows, false, true);
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_units_empty() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_systemctl_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
        });
        let ctx = make_linux_context(config); // which() returns false
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_units_and_systemctl() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
        });
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        assert!(ConfigureSystemd.should_run(&ctx));
    }
}
