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
//! - [`stats`] — result and statistics types
//! - [`update_signal`] — cross-task signalling for config reload
//! - [`scheduler`] — dependency-driven parallel task scheduling

/// Single-resource processing: check state, apply or remove one resource.
pub mod apply;
/// Process-wide cancellation flag for graceful shutdown.
pub mod cancellation;
/// Shared execution context for tasks.
pub mod context;
/// Task dependency graph and cycle detection.
pub mod graph;
mod mode;
mod orchestrate;
mod parallel;
mod stats;
/// Cross-task signalling for config reload.
pub mod update_signal;

/// Dependency-driven parallel task scheduling.
pub(crate) mod scheduler;

pub use cancellation::CancellationToken;
pub use context::Context;
pub use context::ContextOpts;
pub use mode::{ProcessMode, ProcessOpts, ResourceAction};
pub use orchestrate::{process_resource_states, process_resources, process_resources_remove};
pub use stats::{TaskResult, TaskStats};

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests;
