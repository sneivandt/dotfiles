//! Validation tasks.

mod checks;
mod discovery;
mod linters;

pub use checks::{
    RunPSScriptAnalyzer, RunShellcheck, ValidateApmPlugins, ValidateConfigFiles,
    ValidateConfigWarnings, ValidateManifestSync, ValidateSymlinkSources,
};

use crate::infra::config::Diagnostic;
use crate::infra::logging::Output;

pub(crate) fn display_diagnostics(diagnostics: &[Diagnostic], output: &dyn Output) {
    if diagnostics.is_empty() {
        return;
    }

    output.warn(&format!(
        "found {} configuration diagnostic(s):",
        diagnostics.len()
    ));
    for diagnostic in diagnostics {
        output.warn(&format!(
            "  [{}] {} [{}] ({}): {}",
            diagnostic.severity.label(),
            diagnostic.source,
            diagnostic.item,
            diagnostic.code,
            diagnostic.message
        ));
    }
}

#[cfg(test)]
use crate::engine::Task;
#[cfg(test)]
#[cfg(test)]
use discovery::*;
#[cfg(test)]
use linters::*;
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
