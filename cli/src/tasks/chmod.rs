//! Task: apply file permissions.

use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, run_resource_task, task_deps};
use crate::resources::chmod::ChmodResource;

/// Apply file permissions from chmod.toml.
#[derive(Debug)]
pub struct ApplyFilePermissions;

impl Task for ApplyFilePermissions {
    fn name(&self) -> &'static str {
        "Apply file permissions"
    }

    task_deps![super::symlinks::InstallSymlinks];

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_chmod()
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        if !ctx.platform.supports_chmod() {
            return Ok(None);
        }
        let items: Vec<_> = ctx.config_read().chmod.clone();
        if items.is_empty() {
            return Ok(None);
        }
        run_resource_task(
            ctx,
            items,
            |entry, ctx| build_resource(&entry, &ctx.home),
            &ProcessOpts::fix_existing("apply permissions"),
        )
        .map(Some)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let items: Vec<_> = ctx.config_read().chmod.clone();
        if items.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }
        run_resource_task(
            ctx,
            items,
            |entry, ctx| build_resource(&entry, &ctx.home),
            &ProcessOpts::fix_existing("apply permissions"),
        )
    }
}

/// Build a [`ChmodResource`] from a config entry.
///
/// Mode validity is verified by config validation before tasks run, so a
/// parse failure here indicates a bug in the validation pipeline.
#[allow(clippy::expect_used)]
fn build_resource(
    entry: &crate::config::chmod::ChmodEntry,
    home: &std::path::Path,
) -> ChmodResource {
    ChmodResource::from_entry(entry, home)
        .expect("invalid octal mode should have been caught by config validation")
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::chmod::ChmodEntry;
    use crate::tasks::Task;
    use crate::tasks::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ApplyFilePermissions.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_when_guard_passes() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(ApplyFilePermissions.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_chmod_entries_present_on_linux() {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.chmod.push(ChmodEntry {
            mode: "600".to_string(),
            path: "ssh/config".to_string(),
        });
        let ctx = make_linux_context(config);
        assert!(ApplyFilePermissions.should_run(&ctx));
    }
}
