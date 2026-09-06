//! Uninstall command implementation.
use anyhow::Result;
use std::sync::Arc;

use super::RuntimePolicy;
use crate::app::cli::UninstallOpts;
use crate::infra::logging::Logger;

/// Run the uninstall command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    runtime: &RuntimePolicy<'_>,
    _opts: &UninstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    let run_lock = super::prepare_self_update(runtime, log)?;
    let runner = super::CommandRunner::new_with_lock(runtime, log, token, run_lock)?;
    let tasks = runner.uninstall_tasks();
    runner.run(tasks.iter().map(Box::as_ref))
}
