//! Task: configure the login shell.

use anyhow::Result;

use crate::domains::shell::resources::shell::DefaultShellResource;
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};

/// Configure the default shell to zsh.
#[derive(Debug)]
pub struct ConfigureShell;

const NAME: &str = "Default shell";

impl Task for ConfigureShell {
    task_metadata! {
        name: NAME,
        selector: "shell",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().is_linux() && !ctx.is_ci()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if !ctx.which("zsh") {
            return Ok(TaskResult::unmet("zsh not found in PATH"));
        }
        run_resource_task(
            ctx,
            vec![()],
            |(), ctx| {
                DefaultShellResource::new(
                    "zsh".to_string(),
                    ctx.executor_arc(),
                    std::sync::Arc::clone(ctx.env()),
                )
            },
            &ProcessOpts::strict("configure"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Task;
    use crate::infra::platform::Os;
    use crate::test_helpers::{
        ContextBuilder, empty_config, make_linux_context, make_platform_context_with_which,
    };
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_platform_context_with_which(config, Os::Windows, false, true);
        assert!(!ConfigureShell.should_run(&ctx));
    }

    #[test]
    fn run_reports_missing_zsh_as_unmet_work() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config); // which() returns false
        assert!(ConfigureShell.should_run(&ctx));
        assert!(matches!(
            ConfigureShell.run(&ctx).unwrap(),
            TaskResult::Skipped { reason, .. } if reason == "zsh not found in PATH"
        ));
    }

    #[test]
    fn execution_checks_shell_availability_after_assessment() {
        use crate::infra::env::MapEnv;
        use crate::infra::exec::{ExecResult, MockExecutor};
        use crate::infra::platform::Platform;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let ready = Arc::new(AtomicBool::new(false));
        let readiness = Arc::clone(&ready);
        let mut executor = MockExecutor::new();
        executor
            .expect_which()
            .once()
            .withf(|program| program == "zsh")
            .returning(move |_| {
                assert!(
                    readiness.load(Ordering::SeqCst),
                    "readiness is checked during execution"
                );
                true
            });
        executor
            .expect_execute()
            .once()
            .withf(|spec| {
                spec.program() == "getent"
                    && spec.arguments() == ["passwd", "test-user"]
                    && !spec.is_checked()
            })
            .returning(|_| {
                Ok(ExecResult::success(
                    "test-user:x:1000:1000::/home/test-user:/usr/bin/zsh",
                ))
            });
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = crate::test_helpers::make_context(
            config,
            Platform::new(Os::Linux, false),
            Arc::new(executor),
        )
        .with_env(MapEnv::new().with("USER", "test-user").into_handle());

        let assessment = ConfigureShell.assess(&ctx);
        assert!(assessment.is_applicable());
        ready.store(true, Ordering::SeqCst);
        let result = crate::engine::task::execute_assessed(&ConfigureShell, &assessment, &ctx);
        assert_eq!(result.status, crate::infra::logging::TaskStatus::Ok);
    }

    #[test]
    fn should_run_false_when_ci() {
        let config = empty_config(PathBuf::from("/tmp"));
        // Use ContextBuilder.ci(true) — no env var mutation required.
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(true)
            .build();
        assert!(
            !ConfigureShell.should_run(&ctx),
            "should not configure shell in CI"
        );
    }

    #[test]
    fn should_run_true_on_linux_with_zsh_outside_ci() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = ContextBuilder::new(config)
            .os(Os::Linux)
            .which(true)
            .ci(false)
            .build();
        assert!(
            ConfigureShell.should_run(&ctx),
            "should configure shell on Linux when zsh is available and not in CI"
        );
    }
}
