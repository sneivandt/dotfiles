//! Task: configure administrator-enforced Codex requirements.

use anyhow::Result;

use crate::domains::ai::resources::codex_requirements::CodexRequirementsResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskResult, process_resources,
    task_metadata,
};

/// Merge the managed Codex browser policy into the system requirements file.
#[derive(Debug)]
pub struct ConfigureCodexRequirements;

impl Task for ConfigureCodexRequirements {
    task_metadata! {
        name: "Codex requirements",
        selector: "codex-requirements",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().is_linux() && !ctx.is_ci()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        let resource = CodexRequirementsResource::system(ctx.executor_arc());
        matches!(
            resource.current_state(),
            Ok(ResourceState::Missing | ResourceState::Incorrect { .. })
        )
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(
            ctx,
            [CodexRequirementsResource::system(ctx.executor_arc())],
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    #[test]
    fn runs_only_on_linux_outside_ci() {
        for (os, ci, expected) in [
            (Os::Linux, false, true),
            (Os::Linux, true, false),
            (Os::Windows, false, false),
        ] {
            let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
                .os(os)
                .ci(ci)
                .build();
            assert_eq!(ConfigureCodexRequirements.should_run(&ctx), expected);
        }
    }
}
