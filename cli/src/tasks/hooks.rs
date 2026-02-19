use anyhow::{Context as _, Result};

use super::{Context, ProcessOpts, Task, TaskResult, process_resources, process_resources_remove};
use crate::resources::hook::HookFileResource;

/// Discover hook file resources from the hooks/ directory.
fn discover_hooks(ctx: &Context) -> Result<Vec<HookFileResource>> {
    let hooks_src = ctx.hooks_dir();
    let hooks_dst = ctx.root().join(".git/hooks");

    let mut resources = Vec::new();
    for entry in std::fs::read_dir(&hooks_src)
        .with_context(|| format!("reading hooks directory: {}", hooks_src.display()))?
    {
        let entry = entry.with_context(|| format!("reading entry in {}", hooks_src.display()))?;
        let path = entry.path();
        // Only install executable hook files; skip data files like .ini
        if path.is_file() && path.extension().is_none() {
            resources.push(HookFileResource::new(
                path,
                hooks_dst.join(entry.file_name()),
            ));
        }
    }
    Ok(resources)
}

/// Install git hooks from hooks/ into .git/hooks/.
pub struct InstallGitHooks;

impl Task for InstallGitHooks {
    fn name(&self) -> &'static str {
        "Install git hooks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists() && ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
        process_resources(
            ctx,
            resources,
            &ProcessOpts {
                verb: "install hook",
                fix_incorrect: true,
                fix_missing: true,
                bail_on_error: true,
            },
        )
    }
}

/// Remove git hooks that were installed from hooks/.
pub struct UninstallGitHooks;

impl Task for UninstallGitHooks {
    fn name(&self) -> &'static str {
        "Remove git hooks"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists() && ctx.root().join(".git/hooks").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
        process_resources_remove(ctx, resources, "remove hook")
    }
}
