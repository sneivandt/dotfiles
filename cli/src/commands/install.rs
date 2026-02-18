use anyhow::Result;

use crate::cli::{GlobalOpts, InstallOpts};
use crate::config::Config;
use crate::config::profiles;
use crate::logging::Logger;
use crate::platform::Platform;
use crate::tasks::{self, Context, Task};

/// Run the install command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(global: &GlobalOpts, opts: &InstallOpts, log: &Logger) -> Result<()> {
    let platform = Platform::detect();
    let root = resolve_root(global)?;

    let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    log.info(&format!("dotfiles {version}"));

    log.stage("Resolving profile");
    let profile = profiles::resolve_from_args(global.profile.as_deref(), &root, &platform)?;
    log.info(&format!("profile: {}", profile.name));

    log.stage("Loading configuration");
    let config = Config::load(&root, &profile, &platform)?;
    log.info(&format!(
        "loaded {} packages, {} symlinks",
        config.packages.len(),
        config.symlinks.len()
    ));

    let ctx = Context::new(&config, &platform, log, global.dry_run)?;

    // Build the task list
    let all_tasks: Vec<Box<dyn Task>> = vec![
        Box::new(tasks::developer_mode::EnableDeveloperMode),
        Box::new(tasks::sparse_checkout::SparseCheckout),
        Box::new(tasks::update::UpdateRepository),
        Box::new(tasks::hooks::GitHooks),
        Box::new(tasks::git_config::ConfigureGit),
        Box::new(tasks::packages::InstallPackages),
        Box::new(tasks::packages::InstallParu),
        Box::new(tasks::packages::InstallAurPackages),
        Box::new(tasks::symlinks::InstallSymlinks),
        Box::new(tasks::chmod::ApplyFilePermissions),
        Box::new(tasks::shell::ConfigureShell),
        Box::new(tasks::vscode::InstallVsCodeExtensions),
        Box::new(tasks::copilot_skills::InstallCopilotSkills),
        Box::new(tasks::systemd::ConfigureSystemd),
        Box::new(tasks::registry::ApplyRegistry),
    ];

    // Filter by --skip and --only
    let tasks_to_run: Vec<&dyn Task> = all_tasks
        .iter()
        .filter(|t| {
            let name = t.name().to_lowercase();
            if !opts.only.is_empty() {
                return opts.only.iter().any(|o| name.contains(&o.to_lowercase()));
            }
            if !opts.skip.is_empty() {
                return !opts.skip.iter().any(|s| name.contains(&s.to_lowercase()));
            }
            true
        })
        .map(std::convert::AsRef::as_ref)
        .collect();

    for task in tasks_to_run {
        tasks::execute(task, &ctx);
    }

    log.print_summary();

    if log.has_failures() {
        anyhow::bail!("one or more tasks failed");
    }
    Ok(())
}

/// Resolve the dotfiles root directory from CLI arguments or auto-detection.
///
/// # Errors
///
/// Returns an error if the root directory cannot be determined or doesn't exist.
pub fn resolve_root(global: &GlobalOpts) -> Result<std::path::PathBuf> {
    if let Some(ref root) = global.root {
        return Ok(root.clone());
    }

    // Auto-detect: binary location, DOTFILES_ROOT env, or current dir
    if let Ok(root) = std::env::var("DOTFILES_ROOT") {
        return Ok(std::path::PathBuf::from(root));
    }

    // Try to find the repository root from the current binary's location
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        // Check if we're in cli/target/release/ or bin/
        let candidates = [
            parent.join("../../.."), // cli/target/release/ → repo root
            parent.join(".."),       // bin/ → repo root
        ];
        for candidate in &candidates {
            if candidate.join("conf").exists() && candidate.join("symlinks").exists() {
                return Ok(std::fs::canonicalize(candidate)?);
            }
        }
    }

    // Last resort: current directory
    let cwd = std::env::current_dir()?;
    if cwd.join("conf").exists() {
        return Ok(cwd);
    }

    anyhow::bail!("cannot determine dotfiles root. Use --root or set DOTFILES_ROOT env var");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_root_uses_explicit_root() {
        let global = GlobalOpts {
            root: Some(PathBuf::from("/explicit/path")),
            profile: None,
            dry_run: false,
        };

        let result = resolve_root(&global);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from("/explicit/path"));
    }

    #[test]
    fn resolve_root_error_when_not_in_repo() {
        // Use a path that definitely doesn't have conf/symlinks
        let temp_dir = std::env::temp_dir();

        let global = GlobalOpts {
            root: None,
            profile: None,
            dry_run: false,
        };

        // Save and restore current directory
        let original_dir = std::env::current_dir().ok();
        std::env::set_current_dir(&temp_dir).ok();

        let result = resolve_root(&global);

        // Restore directory
        if let Some(dir) = original_dir {
            std::env::set_current_dir(dir).ok();
        }

        // Only check error if DOTFILES_ROOT env var is not set
        if std::env::var("DOTFILES_ROOT").is_err() {
            assert!(result.is_err());
            if let Err(e) = result {
                assert!(e.to_string().contains("cannot determine dotfiles root"));
            }
        }
    }
}
