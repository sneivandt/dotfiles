//! Task: configure file permissions.

use anyhow::Result;

use crate::domains::files::config::chmod::ChmodEntry;
use crate::domains::files::resources::chmod::ChmodResource;
use crate::engine::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_metadata};
use crate::infra::ConfigHandle;

/// Configure file permissions from chmod.toml.
#[derive(Debug)]
pub struct ApplyFilePermissions {
    config: ConfigHandle<Vec<ChmodEntry>>,
}

const NAME: &str = "File permissions";

impl ApplyFilePermissions {
    /// Create the task with a handle to its configuration slice.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<ChmodEntry>>) -> Self {
        Self { config }
    }

    fn process(&self, ctx: &Context, announce: Option<&'static str>) -> Result<TaskResult> {
        let entries = self.config.read().to_vec();
        run_resource_task(
            ctx,
            announce,
            entries,
            |entry, ctx| ChmodResource::from_entry(&entry, ctx.home()),
            &ProcessOpts::fix_existing("configure"),
        )
    }
}

impl Task for ApplyFilePermissions {
    task_metadata! {
        name: NAME,
        selector: "file-permissions",
        deps: [crate::domains::files::symlinks::InstallSymlinks],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().supports_chmod()
    }

    fn run_configured(&self, ctx: &Context) -> Result<TaskResult> {
        self.process(ctx, Some(NAME))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        self.process(ctx, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::files::config::chmod::ChmodEntry;
    use crate::engine::Task;
    use crate::infra::ConfigHandle;
    use crate::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ApplyFilePermissions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_when_guard_passes() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ApplyFilePermissions::new(ConfigHandle::new(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_chmod_entries_present_on_linux() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        let task = ApplyFilePermissions::new(ConfigHandle::new(vec![ChmodEntry::new(
            "600",
            "ssh/config",
        )]));
        assert!(task.should_run(&ctx));
    }
}
