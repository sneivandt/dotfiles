//! Task: configure the login shell.

use std::sync::Arc;

use super::{ProcessOpts, resource_task};
use crate::resources::shell::DefaultShellResource;

resource_task! {
    /// Configure the default shell to zsh.
    pub ConfigureShell {
        name: "Configure default shell",
        deps: [super::packages::InstallPackages],
        guard: |ctx| {
            let is_ci = std::env::var("CI").is_ok();
            ctx.platform.is_linux() && ctx.executor.which("zsh") && !is_ci
        },
        items: |_ctx| vec![()],
        build: |_unit, ctx| DefaultShellResource::new("zsh".to_string(), Arc::clone(&ctx.executor)),
        opts: ProcessOpts::strict("configure"),
    }
}

#[cfg(test)]
#[allow(unsafe_code)] // set_var/remove_var require unsafe since Rust 1.83
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::Os;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{
        empty_config, make_linux_context, make_platform_context_with_which,
    };
    use std::path::PathBuf;

    /// Mutex to serialize tests that mutate the `CI` environment variable.
    static CI_MUTEX: std::sync::LazyLock<std::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

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
    fn should_run_false_when_ci_env_set() {
        let _guard = CI_MUTEX.lock().expect("mutex poisoned");
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        // SAFETY: test-only env var mutation; serialized via CI_MUTEX.
        unsafe { std::env::set_var("CI", "true") };
        let result = ConfigureShell.should_run(&ctx);
        unsafe { std::env::remove_var("CI") };
        assert!(!result, "should not configure shell in CI");
    }

    #[test]
    fn should_run_true_on_linux_with_zsh_outside_ci() {
        let _guard = CI_MUTEX.lock().expect("mutex poisoned");
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_platform_context_with_which(config, Os::Linux, false, true);
        // SAFETY: test-only env var mutation; serialized via CI_MUTEX.
        unsafe { std::env::remove_var("CI") };
        let result = ConfigureShell.should_run(&ctx);
        assert!(
            result,
            "should configure shell on Linux when zsh is available and not in CI"
        );
    }
}
