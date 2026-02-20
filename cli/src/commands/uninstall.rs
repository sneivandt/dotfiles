use anyhow::Result;

use crate::cli::{GlobalOpts, UninstallOpts};
use crate::exec;
use crate::logging::Logger;
use crate::tasks::{self, Context, Task};

/// Run the uninstall command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(global: &GlobalOpts, _opts: &UninstallOpts, log: &Logger) -> Result<()> {
    let executor = exec::SystemExecutor;
    let setup = super::CommandSetup::init(global, log)?;
    let ctx = Context::new(
        std::sync::Arc::new(std::sync::RwLock::new(setup.config)),
        &setup.platform,
        log,
        global.dry_run,
        &executor,
        global.parallel,
    )?;

    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(tasks::symlinks::UninstallSymlinks),
        Box::new(tasks::hooks::UninstallGitHooks),
    ];

    log.stage("Uninstalling");
    super::run_tasks_to_completion(tasks.iter().map(Box::as_ref), &ctx, log)
}
