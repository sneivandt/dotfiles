//! Generic resource processing loop: check state, apply or remove, collect stats.
//!
//! This module is split into sub-modules:
//!
//! - [`apply`] — single-resource processing (`process_single`, `apply_resource`, `remove_single`)
//! - [`context`] — shared execution context for tasks
//! - [`mode`] — processing strategy and action types
//! - [`orchestrate`] — top-level resource orchestration (sequential / parallel dispatch)
//! - [`parallel`] — Rayon-based parallel processing helpers
//! - [`stats`] — result and statistics types

pub mod apply;
pub mod context;
pub mod graph;
mod mode;
mod orchestrate;
mod parallel;
mod stats;
pub mod update_signal;

pub use context::Context;
pub use context::ContextOpts;
pub use mode::{ProcessMode, ProcessOpts, ResourceAction};
pub use orchestrate::{process_resource_states, process_resources, process_resources_remove};
pub use stats::{TaskResult, TaskStats};

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests;
