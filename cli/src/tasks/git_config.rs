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
