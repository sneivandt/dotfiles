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
        if !ctx.dry_run {
            ctx.log.debug("running systemctl --user daemon-reload");
            match ctx.executor.run("systemctl", &["--user", "daemon-reload"]) {
                Ok(_) => ctx.log.debug("daemon-reload succeeded"),
                Err(e) => ctx.log.debug(&format!("daemon-reload failed: {e}")),
            }
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
    use crate::platform::{Os, Platform};
    use crate::resources::test_helpers::MockExecutor;
    use crate::tasks::test_helpers::{
        empty_config, make_context, make_linux_context, make_platform_context_with_which,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn should_run_false_on_windows() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
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
            scope: "user".to_string(),
        });
        let ctx = make_linux_context(config); // which() returns false
        assert!(!ConfigureSystemd.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_units_and_systemctl() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
        });
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        assert!(ConfigureSystemd.should_run(&ctx));
    }

    // ------------------------------------------------------------------
    // ConfigureSystemd::run
    // ------------------------------------------------------------------

    /// Build a context backed by `MockExecutor` for `run()` tests.
    fn make_systemd_context(config: crate::config::Config, executor: MockExecutor) -> Context {
        make_context(
            config,
            Arc::new(Platform::new(Os::Linux, false)),
            Arc::new(executor),
        )
    }

    #[test]
    fn run_calls_daemon_reload_before_enabling_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
        });
        // Ordered responses consumed by the FIFO MockExecutor queue:
        //   1. run("systemctl", ["--user", "daemon-reload"]) → success
        //   2. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) → fail (Missing)
        //   3. run_unchecked("systemctl", ["--user", "enable", "--now", "dunst.service"]) → success
        let executor = MockExecutor::with_responses(vec![
            (true, String::new()),  // daemon-reload
            (false, String::new()), // is-enabled → not enabled → Missing
            (true, String::new()),  // enable → Applied
        ]);
        let ctx = make_systemd_context(config, executor);

        let result = ConfigureSystemd.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok after daemon-reload + enable, got {result:?}"
        );
    }

    #[test]
    fn run_skips_daemon_reload_in_dry_run() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
        });
        // In dry-run mode daemon-reload is NOT called (guarded by `!ctx.dry_run`).
        // current_state() still runs to decide whether change would be needed.
        //   1. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) → fail (Missing)
        let executor = MockExecutor::with_responses(vec![
            (false, String::new()), // is-enabled → Missing
        ]);
        let mut ctx = make_systemd_context(config, executor);
        ctx.dry_run = true;

        let result = ConfigureSystemd.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun when unit is missing in dry-run mode, got {result:?}"
        );
    }
}
