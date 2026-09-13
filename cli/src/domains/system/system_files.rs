//! Task: converge configured privileged system files.

use anyhow::Result;

use crate::domains::system::config::system_files::SystemFile;
use crate::domains::system::resources::system_file::SystemFileResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskResult, process_resources,
    task_metadata,
};
use crate::infra::ConfigHandle;

/// Converge every active entry from `conf/system-files.toml`.
#[derive(Debug)]
pub struct ConfigureSystemFiles {
    files: ConfigHandle<Vec<SystemFile>>,
}

impl ConfigureSystemFiles {
    /// Create the shared system-file task.
    #[must_use]
    pub const fn new(files: ConfigHandle<Vec<SystemFile>>) -> Self {
        Self { files }
    }

    fn resources(&self, ctx: &Context) -> Vec<SystemFileResource> {
        self.files
            .read()
            .iter()
            .cloned()
            .map(|entry| SystemFileResource::new(entry, ctx.executor_arc()))
            .collect()
    }
}

impl Task for ConfigureSystemFiles {
    task_metadata! {
        name: "System files",
        selector: "system-files",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform().is_linux() && !ctx.is_ci() && !self.files.read().is_empty()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        !ctx.is_elevated()
            && self.resources(ctx).iter().any(|resource| {
                matches!(
                    resource.current_state(),
                    Ok(ResourceState::Missing | ResourceState::Incorrect { .. })
                )
            })
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(
            ctx,
            self.resources(ctx),
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::system::config::system_files::MergeStrategy;
    use crate::infra::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    fn configured() -> ConfigHandle<Vec<SystemFile>> {
        ConfigHandle::new(vec![SystemFile {
            target: PathBuf::from("/etc/example"),
            source: PathBuf::from("example"),
            merge: MergeStrategy::Toml,
            origin: Some(PathBuf::from("/repo")),
        }])
    }

    #[test]
    fn runs_only_for_nonempty_linux_configuration_outside_ci() {
        let linux = ContextBuilder::new(empty_config(PathBuf::from("/tmp"))).build();
        let windows = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Windows)
            .build();
        let ci = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .ci(true)
            .build();
        assert!(ConfigureSystemFiles::new(configured()).should_run(&linux));
        assert!(!ConfigureSystemFiles::new(configured()).should_run(&windows));
        assert!(!ConfigureSystemFiles::new(configured()).should_run(&ci));
        assert!(!ConfigureSystemFiles::new(ConfigHandle::new(Vec::new())).should_run(&linux));
    }
}
