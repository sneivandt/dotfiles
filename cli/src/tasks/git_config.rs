use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::git_config::GitConfigResource;

/// Windows-specific git configuration settings.
const GIT_SETTINGS: &[(&str, &str)] = &[
    ("core.autocrlf", "false"),
    ("core.symlinks", "true"),
    ("credential.helper", "manager"),
];

/// Configure git settings (Windows-specific git config).
#[derive(Debug)]
pub struct ConfigureGit;

impl Task for ConfigureGit {
    fn name(&self) -> &'static str {
        "Configure git"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows() && ctx.executor.which("git")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = GIT_SETTINGS.iter().map(|(key, value)| {
            GitConfigResource::new(key.to_string(), value.to_string(), ctx.executor)
        });
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "set git config",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: true,
            },
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::platform::{Os, Platform};
    use crate::tasks::test_helpers::{NoOpExecutor, WhichExecutor, empty_config, make_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Linux, false);
        let executor = WhichExecutor { which_result: true };
        let ctx = make_context(config, &platform, &executor);
        assert!(!ConfigureGit.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_git_not_found() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Windows, false);
        let executor = NoOpExecutor; // which() returns false
        let ctx = make_context(config, &platform, &executor);
        assert!(!ConfigureGit.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows_with_git() {
        let config = empty_config(PathBuf::from("/tmp"));
        let platform = Platform::new(Os::Windows, false);
        let executor = WhichExecutor { which_result: true };
        let ctx = make_context(config, &platform, &executor);
        assert!(ConfigureGit.should_run(&ctx));
    }
}
