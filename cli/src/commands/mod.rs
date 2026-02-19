pub mod install;
pub mod test;
pub mod uninstall;

use anyhow::Result;

use crate::cli::GlobalOpts;
use crate::config::Config;
use crate::config::profiles;
use crate::logging::Logger;
use crate::platform::Platform;
use crate::tasks::{self, Context, Task};

/// Shared state produced by the common command setup sequence.
///
/// Encapsulates platform detection, profile resolution, and configuration
/// loading so that each command does not have to repeat the boilerplate.
pub struct CommandSetup {
    pub platform: Platform,
    pub config: Config,
}

impl CommandSetup {
    /// Detect the platform, resolve the profile, and load all configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the root directory cannot be determined, the profile
    /// cannot be resolved, or any configuration file fails to parse.
    pub fn init(global: &GlobalOpts, log: &Logger) -> Result<Self> {
        let platform = Platform::detect();
        let root = install::resolve_root(global)?;

        log.stage("Resolving profile");
        let profile = profiles::resolve_from_args(global.profile.as_deref(), &root, &platform)?;
        log.info(&format!("profile: {}", profile.name));

        log.stage("Loading configuration");
        let config = Config::load(&root, &profile, &platform)?;

        // Validate configuration and display warnings
        let warnings = config.validate(&platform);
        if !warnings.is_empty() {
            log.warn(&format!(
                "found {} configuration warning(s):",
                warnings.len()
            ));
            for warning in &warnings {
                log.warn(&format!(
                    "  {} [{}]: {}",
                    warning.source, warning.item, warning.message
                ));
            }
        }

        Ok(Self { platform, config })
    }
}

/// Execute every task in order, print the summary, and bail if any task failed.
///
/// # Errors
///
/// Returns an error if one or more tasks recorded a failure.
pub fn run_tasks_to_completion(tasks: &[&dyn Task], ctx: &Context, log: &Logger) -> Result<()> {
    for task in tasks {
        tasks::execute(*task, ctx);
    }

    log.print_summary();

    if log.has_failures() {
        let count = log.failure_count();
        anyhow::bail!("{count} task(s) failed");
    }
    Ok(())
}
