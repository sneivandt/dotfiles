//! Task: configure the login shell.

use std::sync::Arc;

use crate::phases::{ProcessOpts, TaskPhase, resource_task};
use crate::resources::shell::DefaultShellResource;

resource_task! {
    /// Configure the default shell to zsh.
    pub ConfigureShell {
        name: "Configure default shell",
        phase: TaskPhase::Apply,
        deps: [crate::phases::apply::packages::InstallPackages],
        guard: |ctx| {
            ctx.platform.is_linux() && ctx.executor.which("zsh") && !ctx.is_ci
        },
        items: |_ctx| vec![()],
        build: |_unit, ctx| DefaultShellResource::new("zsh".to_string(), Arc::clone(&ctx.executor)),
        opts: ProcessOpts::strict("configure shell"),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::phases::Task;
    use crate::phases::test_helpers::{
        ContextBuilder, empty_config, make_linux_context, make_platform_context_with_which,
    };
    use crate::platform::Os;
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
