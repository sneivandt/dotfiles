//! Task: converge configured privileged system files.

use anyhow::Result;

use crate::domains::system::config::system_files::SystemFile;
use crate::domains::system::resources::system_file::SystemFileResource;
use crate::engine::{
    Context, IntrinsicState, ProcessOpts, ResourceState, Task, TaskResult,
    process_resources_with_cache, task_metadata,
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
            .get()
            .iter()
            .cloned()
            .map(|entry| SystemFileResource::new(entry, ctx.executor_arc()))
            .collect()
    }

    pub(crate) const fn is_applicable(
        platform: crate::infra::platform::Platform,
        has_declarations: bool,
        is_ci: bool,
    ) -> bool {
        platform.is_linux() && has_declarations && !is_ci
    }
}

impl Task for ConfigureSystemFiles {
    task_metadata! {
        name: "System configuration files",
        selector: "system-files",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        Self::is_applicable(ctx.platform(), !self.files.get().is_empty(), ctx.is_ci())
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        self.resources(ctx).iter().any(|resource| {
            !matches!(
                resource.current_state(),
                Ok(ResourceState::Correct | ResourceState::Invalid { .. })
            )
        })
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources_with_cache(
            ctx,
            self.resources(ctx),
            &(),
            |resource, ()| {
                resource.current_state().map(|state| match state {
                    // Discovery cannot read protected destinations. Plan the scoped
                    // assessment/merge, not a claim that their contents are incorrect.
                    ResourceState::Unknown { reason } => {
                        ResourceState::Incorrect { current: reason }
                    }
                    unchanged @ (ResourceState::Missing
                    | ResourceState::Correct
                    | ResourceState::Incorrect { .. }
                    | ResourceState::Invalid { .. }) => unchanged,
                })
            },
            &ProcessOpts::strict("configure").sequential(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::system::config::system_files::MergeStrategy;
    use crate::infra::platform::Os;
    use crate::test_helpers::ContextBuilder;
    use std::fs;
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
        let linux = ContextBuilder::new(PathBuf::from("."))
            .overlay(None)
            .build();
        let windows = ContextBuilder::new(PathBuf::from("."))
            .overlay(None)
            .os(Os::Windows)
            .build();
        let ci = ContextBuilder::new(PathBuf::from("."))
            .overlay(None)
            .ci(true)
            .build();
        assert!(ConfigureSystemFiles::new(configured()).should_run(&linux));
        assert!(!ConfigureSystemFiles::new(configured()).should_run(&windows));
        assert!(!ConfigureSystemFiles::new(configured()).should_run(&ci));
        assert!(!ConfigureSystemFiles::new(ConfigHandle::new(Vec::new())).should_run(&linux));
    }

    #[test]
    fn elevation_planning_does_not_swallow_failed_discovery_or_execute_sudo() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let ctx = ContextBuilder::new(fixture.path().to_path_buf())
            .overlay(None)
            .build();
        let mut files = configured().get().clone();
        for entry in &mut files {
            entry.origin = Some(fixture.path().to_path_buf());
        }
        let task = ConfigureSystemFiles::new(ConfigHandle::new(files));
        assert!(task.needs_elevation(&ctx));
        assert!(
            task.resources(&ctx)
                .iter()
                .all(|resource| resource.current_state().is_err())
        );
    }

    #[test]
    fn elevation_planning_does_not_prompt_for_empty_or_invalid_configuration() {
        let fixture = tempfile::tempdir_in(".").unwrap();
        let ctx = ContextBuilder::new(fixture.path().to_path_buf())
            .overlay(None)
            .build();
        assert!(!ConfigureSystemFiles::new(ConfigHandle::new(Vec::new())).needs_elevation(&ctx));
        fs::create_dir(fixture.path().join("system")).unwrap();
        fs::write(fixture.path().join("system/example"), "managed = [\n").unwrap();
        let mut files = configured().get().clone();
        for entry in &mut files {
            entry.origin = Some(fixture.path().to_path_buf());
        }
        let task = ConfigureSystemFiles::new(ConfigHandle::new(files));
        assert!(!task.needs_elevation(&ctx));
        assert!(task.resources(&ctx).iter().all(|resource| matches!(
            resource.current_state().unwrap(),
            ResourceState::Invalid { .. }
        )));
    }
}
