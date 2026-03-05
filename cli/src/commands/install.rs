//! Install command implementation.
use anyhow::{Context as _, Result};
use std::sync::Arc;

use crate::cli::{GlobalOpts, InstallOpts};
use crate::logging::Logger;
use crate::tasks;

/// Run the install command.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(global: &GlobalOpts, opts: &InstallOpts, log: &Arc<Logger>) -> Result<()> {
    let version = option_env!("DOTFILES_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    log.info(&format!("dotfiles {version}"));

    // Self-update before the task graph — if the binary is replaced, re-exec
    // so all tasks run with the updated code and config parsers.
    // The guard variable prevents an infinite re-exec loop if the new binary
    // also triggers a self-update.
    let root = resolve_root(global)?;
    if std::env::var_os(super::REEXEC_GUARD_VAR).is_none()
        && tasks::self_update::pre_update(&root, &**log, global.dry_run)?
    {
        super::re_exec();
    }

    let runner = super::CommandRunner::new(global, log)?;

    // Filter by --skip and --only
    let all_tasks = tasks::all_install_tasks();
    warn_unmatched_filters(&all_tasks, &opts.only, "--only", &**log);
    warn_unmatched_filters(&all_tasks, &opts.skip, "--skip", &**log);
    runner.run(
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
    )
}

/// Warn about filter values that don't match any known task name.
fn warn_unmatched_filters(
    tasks: &[Box<dyn tasks::Task>],
    filters: &[String],
    flag: &str,
    log: &dyn crate::logging::Output,
) {
    for filter in filters {
        let lower = filter.to_lowercase();
        let matched = tasks
            .iter()
            .any(|t| t.name().to_lowercase().contains(&lower));
        if !matched {
            log.warn(&format!("{flag} '{filter}' did not match any task"));
        }
    }
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

    // ------------------------------------------------------------------
    // warn_unmatched_filters
    // ------------------------------------------------------------------

    #[test]
    fn warn_unmatched_filters_warns_on_no_match() {
        use crate::logging::Logger;
        let log = Logger::new("test");
        let all = tasks::all_install_tasks();

        // "xyznonexistent" should not match any task
        warn_unmatched_filters(&all, &["xyznonexistent".to_string()], "--only", &log);
        // Verification: the function runs without panic; the warning is
        // emitted via log.warn() which is captured by the Logger.
    }

    #[test]
    fn warn_unmatched_filters_silent_on_valid_match() {
        use crate::logging::Logger;
        let log = Logger::new("test");
        let all = tasks::all_install_tasks();

        // "symlink" matches "Install symlinks"
        warn_unmatched_filters(&all, &["symlink".to_string()], "--only", &log);
    }
}
