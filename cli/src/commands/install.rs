use anyhow::{Context as _, Result};
use std::sync::Arc;

use crate::cli::{GlobalOpts, InstallOpts};
use crate::exec;
use crate::logging::Logger;
use crate::tasks::{self, Context};

/// Run the install command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(global: &GlobalOpts, opts: &InstallOpts, log: &Arc<Logger>) -> Result<()> {
    let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    log.info(&format!("dotfiles {version}"));

    let executor: Arc<dyn crate::exec::Executor> = Arc::new(exec::SystemExecutor);
    let setup = super::CommandSetup::init(global, &**log)?;

    let ctx = Context::new(
        std::sync::Arc::new(std::sync::RwLock::new(setup.config)),
        Arc::new(setup.platform),
        Arc::clone(log) as Arc<dyn crate::logging::Log>,
        global.dry_run,
        Arc::clone(&executor),
        global.parallel,
    )?;

    // Filter by --skip and --only
    let all_tasks = tasks::all_install_tasks();
    super::run_tasks_to_completion(
        all_tasks
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
            .map(Box::as_ref),
        &ctx,
        log,
    )
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
                return std::fs::canonicalize(candidate)
                    .context("canonicalizing dotfiles root path");
            }
        }
    }

    // Last resort: current directory
    let cwd = std::env::current_dir().context("determining current directory")?;
    if cwd.join("conf").exists() {
        return Ok(cwd);
    }

    anyhow::bail!("cannot determine dotfiles root. Use --root or set DOTFILES_ROOT env var");
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_root_uses_explicit_root() {
        let global = GlobalOpts {
            root: Some(PathBuf::from("/explicit/path")),
            profile: None,
            dry_run: false,
            parallel: true,
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
            parallel: true,
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
