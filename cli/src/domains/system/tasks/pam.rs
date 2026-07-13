//! Task: configure PAM service files.

use std::sync::Arc;

use anyhow::Result;

use crate::domains::system::config::pam::PamService;
use crate::domains::system::resources::pam::PamServiceResource;
use crate::engine::IntrinsicState;
use crate::engine::{
    Context, Domain, ExecutionPolicy, ProcessOpts, Task, TaskPhase, TaskResult, process_resources,
};
use crate::runtime::ConfigHandle;

/// Install configured PAM service files.
#[derive(Debug)]
pub struct ConfigurePam {
    config: ConfigHandle<Vec<PamService>>,
}

impl ConfigurePam {
    /// Create the task with a handle to the PAM service configuration.
    #[must_use]
    pub const fn new(config: ConfigHandle<Vec<PamService>>) -> Self {
        Self { config }
    }

    fn configured_resources(&self, ctx: &Context) -> Vec<PamServiceResource> {
        self.config
            .read()
            .iter()
            .map(|entry| PamServiceResource::from_entry(entry, Arc::clone(&ctx.executor)))
            .collect()
    }
}

impl Task for ConfigurePam {
    fn name(&self) -> &'static str {
        "Configure PAM services"
    }

    fn phase(&self) -> TaskPhase {
        TaskPhase::Provision
    }

    fn domain(&self) -> Domain {
        Domain::System
    }

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        const POLICIES: &[ExecutionPolicy] = &[
            ExecutionPolicy::PlatformSupported(
                "Linux PAM",
                crate::runtime::platform::Platform::is_linux,
            ),
            ExecutionPolicy::RequiresElevation,
        ];
        POLICIES
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && !self.config.read().is_empty()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        self.configured_resources(ctx).iter().any(|resource| {
            !matches!(
                resource.current_state(),
                Ok(crate::engine::ResourceState::Correct)
            )
        })
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = self.configured_resources(ctx);
        if resources.is_empty() {
            return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
        }

        process_resources(ctx, resources, &ProcessOpts::strict("configure"))
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::domains::system::config::pam::PamService;
    use crate::runtime::ConfigHandle;
    use crate::runtime::platform::Os;
    use crate::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    fn services() -> Vec<PamService> {
        vec![PamService {
            name: "dotfiles-test-nonexistent-service".to_string(),
            content: "auth include login\n".to_string(),
        }]
    }

    fn handle(services: Vec<PamService>) -> ConfigHandle<Vec<PamService>> {
        ConfigHandle::new(services)
    }

    #[test]
    fn should_run_false_on_windows() {
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Windows)
            .build();

        assert!(!ConfigurePam::new(handle(services())).should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_nothing_configured() {
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Linux)
            .build();

        assert!(!ConfigurePam::new(handle(vec![])).should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_configured_services() {
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Linux)
            .build();

        assert!(ConfigurePam::new(handle(services())).should_run(&ctx));
    }

    #[test]
    fn needs_elevation_true_for_missing_service_file() {
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Linux)
            .build();

        assert!(ConfigurePam::new(handle(services())).needs_elevation(&ctx));
    }
}
