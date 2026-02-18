use anyhow::Result;

use crate::cli::{GlobalOpts, UninstallOpts};
use crate::config::Config;
use crate::config::profiles;
use crate::logging::Logger;
use crate::platform::Platform;
use crate::tasks::{self, Context, Task};

/// Run the uninstall command.
pub fn run(global: &GlobalOpts, _opts: &UninstallOpts, log: &Logger) -> Result<()> {
    let platform = Platform::detect();
    let root = super::install::resolve_root(global)?;

    log.stage("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), &root, &platform)?;
    log.info(&format!("profile: {}", profile.name));

    log.stage("Loading configuration");
    let config = Config::load(&root, &profile, &platform)?;

    let ctx = Context::new(&config, &platform, log, global.dry_run)?;

    let tasks: Vec<Box<dyn Task>> = vec![
        Box::new(tasks::symlinks::UninstallSymlinks),
        Box::new(tasks::hooks::UninstallHooks),
    ];

    log.stage("Uninstalling");
    for task in &tasks {
        tasks::execute(task.as_ref(), &ctx);
    }

    log.print_summary();

    if log.has_failures() {
        anyhow::bail!("one or more tasks failed");
    }
    Ok(())
}
