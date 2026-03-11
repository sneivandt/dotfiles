//! Task: configure systemd user units.

use std::sync::Arc;

use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_deps};
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

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        if !(ctx.platform.supports_systemd()
            && !ctx.config_read().units.is_empty()
            && ctx.executor.which("systemctl"))
        {
            return Ok(None);
        }
        let items: Vec<_> = ctx.config_read().units.clone();
        daemon_reload_if_live(ctx);
        run_resource_task(
            ctx,
            items,
            |entry, ctx| SystemdUnitResource::from_entry(&entry, Arc::clone(&ctx.executor)),
            &ProcessOpts::install_missing("enable"),
        )
        .map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items: Vec<_> = ctx.config_read().units.clone();
        if items.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        daemon_reload_if_live(ctx);
        run_resource_task(
            ctx,
            items,
            |entry, ctx| SystemdUnitResource::from_entry(&entry, Arc::clone(&ctx.executor)),
            &ProcessOpts::install_missing("enable"),
        )
    }
}

/// Run `systemctl --user daemon-reload` unless the context is in dry-run mode.
fn daemon_reload_if_live(ctx: &Context) {
    if !ctx.dry_run {
        ctx.log.debug("running systemctl --user daemon-reload");
        match ctx.executor.run("systemctl", &["--user", "daemon-reload"]) {
            Ok(_) => ctx.log.debug("daemon-reload succeeded"),
            Err(e) => ctx.log.debug(&format!("daemon-reload failed: {e}")),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::systemd_units::SystemdUnit;
    use crate::exec::test_helpers::TestExecutor;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{
        empty_config, make_context, make_linux_context, make_platform_context_with_which,
    };
    use crate::tasks::{Context, Task, TaskResult};
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

    /// Build a context backed by `TestExecutor` for `run()` tests.
    fn make_systemd_context(config: crate::config::Config, executor: TestExecutor) -> Context {
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

    #[test]
    fn run_calls_daemon_reload_before_enabling_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
        });
        // Ordered responses consumed by the FIFO TestExecutor queue:
        //   1. run("systemctl", ["--user", "daemon-reload"]) → success
        //   2. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) → fail (Missing)
        //   3. run_unchecked("systemctl", ["--user", "enable", "--now", "dunst.service"]) → success
        let executor = TestExecutor::with_responses(vec![
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
        let executor = TestExecutor::with_responses(vec![
            (false, String::new()), // is-enabled → Missing
        ]);
        let mut ctx = make_systemd_context(config, executor);
        ctx = ctx.with_dry_run(true);

        let result = ConfigureSystemd.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun when unit is missing in dry-run mode, got {result:?}"
        );
    }
}
