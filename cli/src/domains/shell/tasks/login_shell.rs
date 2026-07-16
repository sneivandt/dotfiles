//! Task: configure the login shell.

use crate::domains::shell::resources::shell::DefaultShellResource;
use crate::engine::{Domain, PlatformCapability, ProcessOpts, TaskPhase, resource_task};

resource_task! {
    /// Configure the default shell to zsh.
    pub ConfigureShell {
        name: "Configure default shell",
        phase: TaskPhase::Provision,
        domain: Domain::Shell,
        policy: [PlatformCapability::LinuxShell.policy()],
        guard: |ctx| {
            let system = ctx.system();
            system.platform().is_linux() && system.which("zsh") && !system.is_ci()
        },
        items: |_ctx| vec![()],
        build: |_unit, ctx| DefaultShellResource::new("zsh".to_string(), ctx.system().executor_arc()),
        opts: ProcessOpts::strict("configure"),
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
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
    fn should_run_false_when_zsh_not_found() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config); // which() returns false
        assert!(!ConfigureShell.should_run(&ctx));
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
