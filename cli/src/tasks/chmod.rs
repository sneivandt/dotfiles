use anyhow::Result;
use std::any::TypeId;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::chmod::ChmodResource;

/// Apply file permissions from chmod.ini.
#[derive(Debug)]
pub struct ApplyFilePermissions;

impl Task for ApplyFilePermissions {
    fn name(&self) -> &'static str {
        "Apply file permissions"
    }

    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::symlinks::InstallSymlinks>()];
        DEPS
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_chmod() && !ctx.config_read().chmod.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let entries: Vec<_> = ctx.config_read().chmod.clone();
        let resources = entries
            .iter()
            .map(|entry| ChmodResource::from_entry(entry, &ctx.home));
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "chmod",
                fix_incorrect: true,
                fix_missing: false,
                bail_on_error: true,
            },
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::chmod::ChmodEntry;
    use crate::tasks::test_helpers::{empty_config, make_linux_context, make_windows_context};
    use std::path::PathBuf;

    #[test]
    fn should_run_false_on_windows() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_windows_context(config);
        assert!(!ApplyFilePermissions.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_chmod_empty() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = make_linux_context(config);
        assert!(!ApplyFilePermissions.should_run(&ctx));
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
