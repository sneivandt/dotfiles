//! Editor tasks.

mod vscode_extensions;

pub use vscode_extensions::InstallVsCodeExtensions;

#[cfg(test)]
use crate::engine::{Task, TaskResult};

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
