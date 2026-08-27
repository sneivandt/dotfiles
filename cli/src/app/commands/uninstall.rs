//! Uninstall command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, UninstallOpts};
use crate::infra::logging::Logger;

/// Run the uninstall command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    global: &GlobalOpts,
    _opts: &UninstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    let run_lock = super::prepare_self_update(global, log)?;
    let runner = super::CommandRunner::new_with_lock(global, log, token, run_lock)?;
    let tasks = runner.uninstall_tasks();
    let selected = if let Some(selectors) = runner.recovery_selectors() {
        crate::app::recovery::select_tasks(&tasks, &[], selectors, &[])?
    } else {
        tasks.iter().map(Box::as_ref).collect()
    };
    runner.run(selected)
}
