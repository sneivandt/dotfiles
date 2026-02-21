use anyhow::Result;
use std::any::TypeId;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::systemd_unit::SystemdUnitResource;

/// Enable and start systemd user units.
#[derive(Debug)]
pub struct ConfigureSystemd;

impl Task for ConfigureSystemd {
    fn name(&self) -> &'static str {
        "Configure systemd units"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::symlinks::InstallSymlinks>()];
        DEPS
    }

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
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "enable",
                fix_incorrect: false,
                fix_missing: true,
                bail_on_error: false,
            },
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::systemd_units::SystemdUnit;
    use crate::exec::Executor;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, WhichExecutor, empty_config, make_context};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn should_run_false_on_windows() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
        });
        let platform = Arc::new(Platform::new(Os::Windows, false));
        let executor: Arc<dyn Executor> = Arc::new(WhichExecutor { which_result: true });
        let ctx = make_context(config, platform, executor);
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_units_empty() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(WhichExecutor { which_result: true });
        let ctx = make_context(config, platform, executor);
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_systemctl_not_found() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
        });
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(NoOpExecutor); // which() returns false
        let ctx = make_context(config, platform, executor);
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_units_and_systemctl() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
        });
        let platform = Arc::new(Platform::new(Os::Linux, false));
        let executor: Arc<dyn Executor> = Arc::new(WhichExecutor { which_result: true });
        let ctx = make_context(config, platform, executor);
        assert!(ConfigureSystemd.should_run(&ctx));
    }
}
