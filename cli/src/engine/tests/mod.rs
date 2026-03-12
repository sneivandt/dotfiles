mod apply_tests;
mod orchestrate;
mod stats;

use crate::engine::mode::ProcessOpts;
use crate::error::ResourceError;
use crate::resources::{Applicable, Resource, ResourceChange, ResourceState};
use crate::tasks::test_helpers::make_static_context;

// -----------------------------------------------------------------------
// Test doubles
// -----------------------------------------------------------------------

/// A configurable mock resource for testing the processing pipeline.
pub struct MockResource {
    state_result: Result<ResourceState, String>,
    apply_result: Result<ResourceChange, String>,
    remove_result: Result<ResourceChange, String>,
    desc: String,
}

impl MockResource {
    pub fn new(state: ResourceState) -> Self {
        Self {
            state_result: Ok(state),
            apply_result: Ok(ResourceChange::Applied),
            remove_result: Ok(ResourceChange::Applied),
            desc: "mock resource".to_string(),
        }
    }

    pub fn with_desc(mut self, desc: impl Into<String>) -> Self {
        self.desc = desc.into();
        self
    }

    pub fn with_state_error(mut self, err: impl Into<String>) -> Self {
        self.state_result = Err(err.into());
        self
    }

    pub fn with_apply(mut self, result: Result<ResourceChange, String>) -> Self {
        self.apply_result = result;
        self
    }

    pub fn with_remove(mut self, result: Result<ResourceChange, String>) -> Self {
        self.remove_result = result;
        self
    }
}

impl Applicable for MockResource {
    fn description(&self) -> String {
        self.desc.clone()
    }

    fn apply(&self) -> anyhow::Result<ResourceChange> {
        self.apply_result
            .clone()
            .map_err(|s| anyhow::anyhow!("{s}"))
    }

    fn remove(&self) -> anyhow::Result<ResourceChange> {
        self.remove_result
            .clone()
            .map_err(|s| anyhow::anyhow!("{s}"))
    }
}

impl Resource for MockResource {
    fn current_state(&self) -> anyhow::Result<ResourceState> {
        self.state_result
            .clone()
            .map_err(|s| anyhow::anyhow!("{s}"))
    }
}

/// A mock resource that returns a typed [`ResourceError`] from `apply()`.
pub struct TypedErrorResource {
    pub error_variant: &'static str,
}

impl Applicable for TypedErrorResource {
    fn description(&self) -> String {
        "typed-error resource".to_string()
    }

    fn apply(&self) -> anyhow::Result<ResourceChange> {
        match self.error_variant {
            "command_failed" => Err(ResourceError::command_failed("pacman", "exit code 1").into()),
            "permission_denied" => Err(ResourceError::permission_denied("/etc/secure").into()),
            "conflicting_state" => Err(ResourceError::conflicting_state("test", "a", "b").into()),
            "not_supported" => Err(ResourceError::not_supported("linux only").into()),
            other => Err(anyhow::anyhow!("unknown error variant: {other}")),
        }
    }

    fn remove(&self) -> anyhow::Result<ResourceChange> {
        Ok(ResourceChange::Applied)
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

pub fn test_context(
    config: crate::config::Config,
) -> (
    crate::tasks::Context,
    std::sync::Arc<crate::logging::Logger>,
) {
    make_static_context(config)
}

pub fn dry_run_context(
    config: crate::config::Config,
) -> (
    crate::tasks::Context,
    std::sync::Arc<crate::logging::Logger>,
) {
    let (mut ctx, log) = test_context(config);
    ctx = ctx.with_dry_run(true);
    (ctx, log)
}

pub fn parallel_context(
    config: crate::config::Config,
) -> (
    crate::tasks::Context,
    std::sync::Arc<crate::logging::Logger>,
) {
    let (mut ctx, log) = test_context(config);
    ctx = ctx.with_parallel(true);
    (ctx, log)
}

pub fn default_opts() -> ProcessOpts<'static> {
    ProcessOpts::lenient("install")
}

pub fn bail_opts() -> ProcessOpts<'static> {
    ProcessOpts::strict("install")
}
