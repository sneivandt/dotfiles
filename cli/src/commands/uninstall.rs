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
    let setup = super::CommandSetup::init(global, log)?;
    let ctx = Context::new(
        &setup.config,
        &setup.platform,
        log,
        global.dry_run,
        &exec::SystemExecutor,
    )?;

    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(tasks::symlinks::UninstallSymlinks),
        Box::new(tasks::hooks::UninstallGitHooks),
    ];

    log.stage("Uninstalling");
    let task_refs: Vec<&dyn Task> = tasks.iter().map(std::convert::AsRef::as_ref).collect();
    super::run_tasks_to_completion(&task_refs, &ctx, log)
}
