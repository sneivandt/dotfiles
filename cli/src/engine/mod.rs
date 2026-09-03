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
/// Dependency-driven parallel task scheduling.
pub(crate) mod scheduler;
mod stats;

/// Generic resource contract shared by all concrete domain resources.
pub mod resource;
/// Generic task contract, metadata vocabulary, macros, and executor.
pub mod task;

pub use crate::infra::cancellation::CancellationToken;
pub use context::Context;
pub use context::ContextOpts;
pub use mode::ProcessMode;
pub use mode::ProcessOpts;
pub(crate) use operation::{Operation, OperationState, process_operation};
pub use orchestrate::{process_resources, process_resources_remove, process_resources_with_cache};
pub use resource::{
    IntrinsicState, IntrinsicStateProvider, RemovableResource, Resource, ResourceChange,
    ResourceResult, ResourceState, ResourceStateProvider, SkipKind,
};
pub use stats::{TaskResult, TaskStats};
#[cfg(test)]
pub use task::requires_elevation;
pub use task::{
    Task, TaskAssessment, TaskId, TaskMeta, TaskVisibility, TaskWithExtraDeps, execute,
};
pub(crate) use task::{
    TaskOutcome, run_batch_resource_task, run_resource_task, task_deps, task_metadata,
};

#[cfg(test)]
mod tests;
