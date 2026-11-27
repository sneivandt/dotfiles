//! Validation tasks.

mod checks;
mod discovery;

pub(crate) use checks::{
    ValidateApmPlugins, ValidateConfigFiles, ValidateConfigWarnings, ValidateSymlinkSources,
};

use crate::infra::config::Diagnostic;
use crate::infra::logging::Output;
use crate::infra::logging::OutputExt as _;

pub(crate) fn display_diagnostics(diagnostics: &[Diagnostic], output: &dyn Output) {
    if diagnostics.is_empty() {
        return;
    }

    let count = diagnostics.len();
    let unit = if count == 1 { "issue" } else { "issues" };
    output.warn(format!("found {count} configuration {unit}:"));
    for diagnostic in diagnostics {
        output.warn_continuation(format!(
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
use discovery::*;

#[cfg(test)]
mod tests;
