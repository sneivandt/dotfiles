//! Task: configure systemd user units.

use std::sync::Arc;

use crate::resources::systemd_unit::SystemdUnitResource;
use crate::tasks::{ProcessOpts, TaskPhase, resource_task};

resource_task! {
    /// Enable and start systemd user units.
    pub ConfigureSystemd {
        name: "Configure systemd units",
        phase: TaskPhase::Apply,
        deps: [crate::tasks::user::symlinks::InstallSymlinks],
        guard: |ctx| {
            ctx.platform.supports_systemd()
                && !ctx.config_read().units.is_empty()
                && ctx.executor.which("systemctl")
        },
        setup: |ctx| {
            if !ctx.dry_run {
                ctx.log.debug("running systemctl --user daemon-reload");
                match ctx.executor.run("systemctl", &["--user", "daemon-reload"]) {
                    Ok(_) => ctx.log.debug("daemon-reload succeeded"),
                    Err(e) => ctx.debug_fmt(|| format!("daemon-reload failed: {e}")),
                }
            }
        },
        items: |ctx| ctx.config_read().units.clone(),
        build: |entry, ctx| SystemdUnitResource::from_entry(&entry, Arc::clone(&ctx.executor)),
        opts: ProcessOpts::install_missing("enable"),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::systemd_units::SystemdUnit;
    use crate::exec::{ExecResult, MockExecutor};
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{
        empty_config, make_context, make_linux_context, make_platform_context_with_which,
    };
    use crate::tasks::{Context, Task, TaskResult};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn ok_result() -> ExecResult {
        ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    fn fail_result() -> ExecResult {
        ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            success: false,
            code: Some(1),
        }
    }

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
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

    #[test]
    fn run_calls_daemon_reload_before_enabling_unit() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.units.push(SystemdUnit {
            name: "dunst.service".to_string(),
            scope: "user".to_string(),
        });
        // Ordered expectations:
        //   1. run("systemctl", ["--user", "daemon-reload"]) → success
        //   2. run_unchecked("systemctl", ["--user", "is-enabled", "dunst.service"]) → fail (Missing)
        //   3. run_unchecked("systemctl", ["--user", "enable", "--now", "dunst.service"]) → success
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_run()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result()));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(fail_result()));
        mock.expect_run_unchecked()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(ok_result()));
        let ctx = make_systemd_context(config, mock);

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
        let mut mock = MockExecutor::new();
        mock.expect_run_unchecked()
            .once()
            .returning(|_, _| Ok(fail_result()));
        let mut ctx = make_systemd_context(config, mock);
        ctx = ctx.with_dry_run(true);

        let result = ConfigureSystemd.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun when unit is missing in dry-run mode, got {result:?}"
        );
    }
}
