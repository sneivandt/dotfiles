//! Check command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{CheckOpts, GlobalOpts};
use crate::app::filter::apply_task_filters;
use crate::app::validation::{
    RunPSScriptAnalyzer, RunShellcheck, ValidateApmPlugins, ValidateConfigFiles,
    ValidateConfigWarnings, ValidateManifestSync, ValidateSymlinkSources,
};
use crate::engine::Task;
use crate::infra::logging::Logger;

/// Run repository validation checks.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration validation, or script checks fail.
pub fn run(
    global: &GlobalOpts,
    opts: &CheckOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    let runner = super::CommandRunner::new(global, log, token)?;
    let tasks = validation_tasks(runner.config_handle());
    let filtered = if let Some(selectors) = runner.recovery_selectors() {
        if !opts.only.is_empty() || !opts.skip.is_empty() {
            anyhow::bail!("--retry-failed cannot be combined with --only or --skip");
        }
        crate::app::recovery::select_tasks(&tasks, &[], selectors, &[])?
    } else {
        apply_task_filters(&tasks, &[], &opts.only, &opts.skip, opts.with_deps, log)?
    };
    runner.run(filtered)
}

/// Build the complete task set used by the `check` command.
#[must_use]
pub(crate) fn validation_tasks(
    handle: crate::infra::ConfigHandle<crate::app::config::Config>,
) -> Vec<Box<dyn Task>> {
    vec![
        Box::new(ValidateConfigWarnings::new(handle.clone())),
        Box::new(ValidateSymlinkSources::new(handle)),
        Box::new(ValidateConfigFiles),
        Box::new(ValidateManifestSync),
        Box::new(ValidateApmPlugins),
        Box::new(RunShellcheck),
        Box::new(RunPSScriptAnalyzer),
    ]
}
