//! Test command implementation (validates configuration).
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, TestOpts};
use crate::app::validation::{
    RunPSScriptAnalyzer, RunShellcheck, ValidateApmPlugins, ValidateConfigFiles,
    ValidateConfigWarnings, ValidateManifestSync, ValidateSymlinkSources,
};
use crate::engine::Task;
use crate::infra::logging::Logger;

/// Run the test/validation command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration validation, or script checks fail.
pub fn run(
    global: &GlobalOpts,
    _opts: &TestOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    let runner = super::CommandRunner::new(global, log, token)?;
    let handle = runner.config_handle();
    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(ValidateConfigWarnings::new(handle.clone())),
        Box::new(ValidateSymlinkSources::new(handle)),
        Box::new(ValidateConfigFiles),
        Box::new(ValidateManifestSync),
        Box::new(ValidateApmPlugins),
        Box::new(RunShellcheck),
        Box::new(RunPSScriptAnalyzer),
    ];
    runner.run(tasks.iter().map(Box::as_ref))
}
