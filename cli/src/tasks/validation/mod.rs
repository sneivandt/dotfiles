//! Validation tasks.

mod checks;

pub use checks::{
    RunPSScriptAnalyzer, RunShellcheck, ValidateConfigFiles, ValidateConfigWarnings,
    ValidateManifestSync, ValidateSymlinkSources,
};

#[cfg(test)]
use crate::tasks::Task;
#[cfg(test)]
use checks::*;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
