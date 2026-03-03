//! Test command implementation (validates configuration).
use anyhow::Result;
use std::sync::Arc;

use crate::cli::{GlobalOpts, TestOpts};
use crate::logging::Logger;
use crate::tasks::Task;
use crate::tasks::validation::{
    RunPSScriptAnalyzer, RunShellcheck, ValidateConfigFiles, ValidateSymlinkSources,
};

/// Run the test/validation command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration validation, or script checks fail.
pub fn run(global: &GlobalOpts, _opts: &TestOpts, log: &Arc<Logger>) -> Result<()> {
    let runner = super::CommandRunner::new(global, log)?;
    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(ValidateSymlinkSources),
        Box::new(ValidateConfigFiles),
        Box::new(RunShellcheck),
        Box::new(RunPSScriptAnalyzer),
    ];
    runner.run(tasks.iter().map(Box::as_ref))
}
