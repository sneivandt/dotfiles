//! Test command implementation (validates configuration).
use anyhow::Result;
use std::sync::Arc;

use crate::cli::{GlobalOpts, TestOpts};
use crate::logging::Logger;
use crate::tasks::Task;
use crate::tasks::validation::{
    RunPSScriptAnalyzer, RunShellcheck, ValidateConfigFiles, ValidateConfigWarnings,
    ValidateManifestSync, ValidateSymlinkSources,
};

/// Run the test/validation command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration validation, or script checks fail.
pub fn run(global: &GlobalOpts, opts: &TestOpts, log: &Arc<Logger>) -> Result<()> {
    let _ = opts;
    let runner = super::CommandRunner::new(global, log)?;
    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(ValidateConfigWarnings),
        Box::new(ValidateSymlinkSources),
        Box::new(ValidateConfigFiles),
        Box::new(ValidateManifestSync),
        Box::new(RunShellcheck),
        Box::new(RunPSScriptAnalyzer),
    ];
    runner.run(tasks.iter().map(Box::as_ref))
}
