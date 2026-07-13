//! Execution engine: resource processing, dependency graphs, and task orchestration.
//!
//! This module is split into sub-modules:
//!
//! - [`apply`] — single-resource processing (`process_single`, `apply_resource`, `remove_single`)
//! - [`context`] — shared execution context for tasks
//! - [`graph`] — task dependency graph and cycle detection
//! - [`mode`] — processing strategy and action types
//! - [`orchestrate`] — top-level resource orchestration (sequential / parallel dispatch)
//! - [`parallel`] — Rayon-based parallel processing helpers
//! - [`plan`] — pure resource plan/diff construction
//! - [`stats`] — result and statistics types
//! - [`update_signal`] — cross-task signalling for config reload
//! - [`scheduler`] — dependency-driven parallel task scheduling

/// Single-resource processing: check state, apply or remove one resource.
pub mod apply;
/// Shared execution context for tasks.
pub mod context;
/// Task dependency graph and cycle detection.
pub mod graph;
mod mode;
mod operation;
mod orchestrate;
mod parallel;
pub(crate) mod plan;
mod stats;
/// Cross-task signalling for config reload.
pub mod update_signal;

/// Dependency-driven parallel task scheduling.
pub(crate) mod scheduler;

/// Generic resource contract shared by all concrete domain resources.
pub mod resource;
/// Generic task contract, metadata vocabulary, macros, and executor.
pub mod task;

pub use crate::runtime::cancellation::CancellationToken;
pub use context::Context;
pub use context::ContextOpts;
pub use mode::ProcessOpts;
#[cfg(any(test, feature = "internal-api", doctest))]
pub use mode::{ProcessMode, ResourceAction};
pub(crate) use operation::{Operation, OperationState, process_operation};
pub use orchestrate::{
    process_resources, process_resources_remove, process_resources_with_provider,
};
pub use resource::{
    IntrinsicState, IntrinsicStateProvider, PreloadedStateProvider, Resource, ResourceChange,
    ResourceResult, ResourceState, ResourceStateProvider,
};
pub use stats::{TaskResult, TaskStats};
pub use task::{
    Domain, ExecutionPolicy, PlatformCapability, Task, TaskId, TaskPhase, TaskWithExtraDeps,
    execute,
};
pub(crate) use task::{
    config_resource_task, execution_policies_impl, process_config_resources,
    process_config_resources_with_provider, process_resources_with_borrowed_cache, resource_task,
    task_deps, task_metadata,
};
pub use update_signal::UpdateSignal;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
