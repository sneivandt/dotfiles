//! Remove command implementation.
use anyhow::Result;
use std::sync::Arc;

use super::RuntimePolicy;
use crate::app::cli::RemoveOpts;
use crate::app::filter::apply_task_filters;
use crate::infra::logging::Logger;

/// Run the remove command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    runtime: &RuntimePolicy<'_>,
    opts: &RemoveOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    super::record_scope(log, &opts.filters, false, false);
    let startup = super::runner::CommandStartup::for_mutation(runtime, log)?;
    let checkout_snapshot = startup.checkout_snapshot()?;
    let runner = super::CommandRunner::from_startup(runtime, log, token, startup)?;
    let tasks = runner.remove_tasks(opts.restore_missing);
    let overlay_tasks = runner.overlay_script_removal_tasks();
    let filtered = apply_task_filters(
        &tasks,
        &overlay_tasks,
        &opts.filters.only,
        &opts.filters.skip,
        false,
        log,
    )?;
    checkout_snapshot.ensure_current()?;
    runner.run(filtered)
}
