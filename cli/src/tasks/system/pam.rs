//! Task: configure PAM service files.

use std::sync::Arc;

use anyhow::Result;

use crate::resources::IntrinsicState;
use crate::resources::pam::PamServiceResource;
use crate::tasks::{
    Context, Domain, ExecutionPolicy, ProcessOpts, Task, TaskPhase, TaskResult, process_resources,
    task_metadata,
};

/// Install configured PAM service files.
#[derive(Debug)]
pub struct ConfigurePam;

impl ConfigurePam {
    fn configured_resources(ctx: &Context) -> Vec<PamServiceResource> {
        ctx.config_read()
            .pam_services
            .iter()
            .map(|entry| PamServiceResource::from_entry(entry, Arc::clone(&ctx.executor)))
            .collect()
    }
}

impl Task for ConfigurePam {
    task_metadata! {
        name: "Configure PAM services",
        phase: TaskPhase::Provision,
        domain: Domain::System,
        policy: [
            ExecutionPolicy::PlatformSupported("Linux PAM", crate::platform::Platform::is_linux),
            ExecutionPolicy::RequiresElevation,
        ],
        deps: [
            crate::tasks::packages::InstallPackages,
            crate::tasks::packages::InstallAurPackages,
        ],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_linux() && !ctx.config_read().pam_services.is_empty()
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        Self::configured_resources(ctx).iter().any(|resource| {
            !matches!(
                resource.current_state(),
                Ok(crate::resources::ResourceState::Correct)
            )
        })
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = Self::configured_resources(ctx);
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
    use crate::config::pam::PamService;
    use crate::platform::Os;
    use crate::tasks::test_helpers::{ContextBuilder, empty_config};
    use std::path::PathBuf;

    fn config_with_pam_service() -> crate::config::Config {
        let mut config = empty_config(PathBuf::from("/tmp"));
        config.pam_services.push(PamService {
            name: "dotfiles-test-nonexistent-service".to_string(),
            content: "auth include login\n".to_string(),
        });
        config
    }

    #[test]
    fn should_run_false_on_windows() {
        let ctx = ContextBuilder::new(config_with_pam_service())
            .os(Os::Windows)
            .build();

        assert!(!ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_false_when_nothing_configured() {
        let ctx = ContextBuilder::new(empty_config(PathBuf::from("/tmp")))
            .os(Os::Linux)
            .build();

        assert!(!ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn should_run_true_on_linux_with_configured_services() {
        let ctx = ContextBuilder::new(config_with_pam_service())
            .os(Os::Linux)
            .build();

        assert!(ConfigurePam.should_run(&ctx));
    }

    #[test]
    fn needs_elevation_true_for_missing_service_file() {
        let ctx = ContextBuilder::new(config_with_pam_service())
            .os(Os::Linux)
            .build();

        assert!(ConfigurePam.needs_elevation(&ctx));
    }
}
