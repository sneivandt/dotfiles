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
        ctx.platform.is_windows()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = GIT_SETTINGS
            .iter()
            .map(|(key, value)| GitConfigResource::new(key.to_string(), value.to_string()));
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
    use crate::tasks::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ConfigureGit.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(ConfigureGit.should_run(&ctx));
    }
}
