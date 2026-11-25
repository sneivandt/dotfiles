//! Consolidated integration test binary for CLI commands, task execution,
//! and end-to-end lifecycle contracts.
//!
//! Standalone integration binaries remain for targets invoked individually by
//! CI and mutation scripts (`config_drift`, `docs_contracts`, and
//! `domain_boundaries`).
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests use panicking assertions for clear failures"
)]

#[path = "../common/mod.rs"]
mod common;

mod audit_lifecycle;
mod behavioral_ci;
mod check_command;
mod completion_contracts;
mod e2e_apply;
mod lifecycle_contracts;
mod list_command;
mod remove_command;
mod task_execution;
mod task_output;
mod update_command;
