use anyhow::Result;

use super::{Context, ProcessOpts, Task, TaskResult, process_resources};
use crate::resources::chmod::ChmodResource;

/// Apply file permissions from chmod.ini.
#[derive(Debug)]
pub struct ApplyFilePermissions;

impl Task for ApplyFilePermissions {
    fn name(&self) -> &'static str {
        "Apply file permissions"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.supports_chmod() && !ctx.config.chmod.is_empty()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = ctx
            .config
            .chmod
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
