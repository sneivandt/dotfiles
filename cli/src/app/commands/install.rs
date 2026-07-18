//! Install command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, InstallOpts};
use crate::app::filter;
use crate::engine::{Task, TaskPhase};
use crate::infra::logging::Logger;

/// Install pipeline behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunMode {
    /// Converge to declared state without advancing locked versions.
    Install,
    /// Converge and advance locked dependency versions.
    Update,
}

impl RunMode {
    pub(super) const fn advances_versions(self) -> bool {
        matches!(self, Self::Update)
    }

    const fn includes_phase(self, phase: TaskPhase) -> bool {
        self.advances_versions() || !matches!(phase, TaskPhase::Update)
    }
}

/// Run the install command.
///
/// Converges the system to the declared state without advancing locked
/// dependency versions (see [`crate::app::commands::update`] for that).
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    global: &GlobalOpts,
    opts: &InstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    run_pipeline(global, opts, log, token, RunMode::Install)
}

/// Shared implementation behind both `install` and `update`.
///
/// The two commands run the identical task graph; `mode` determines whether
/// version-advancing tasks additionally move locked refs forward.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub(crate) fn run_pipeline(
    global: &GlobalOpts,
    opts: &InstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
    mode: RunMode,
) -> Result<()> {
    super::prepare_self_update(global, log)?;

    let runner = super::CommandRunner::new(global, log, token)?.with_run_mode(mode);

    // Build the static task list. Dynamic overlay scripts are rebuilt after
    // Sync so they observe configuration pulled and reloaded in this run.
    let mut all_tasks = runner.install_tasks();

    // Version-advancing tasks (the `Update` phase) are only scheduled by the
    // `update` command.  Drop them here for `install` so the `Updating
    // dependencies` phase is empty (its header is suppressed) and so `--only`/
    // `--skip` warnings and matching below reason about the command-eligible
    // task set rather than tasks that could never run.
    all_tasks.retain(|task| mode.includes_phase(task.phase()));

    let startup_overlay_tasks = runner.overlay_script_tasks();
    let known_task_refs: Vec<&dyn Task> = all_tasks
        .iter()
        .chain(&startup_overlay_tasks)
        .map(Box::as_ref)
        .collect();
    if !log.is_verbose()
        && (has_unmatched_filter(&known_task_refs, &opts.only)
            || has_unmatched_filter(&known_task_refs, &opts.skip))
    {
        log.separate_from_startup();
    }
    filter::warn_unmatched_filters(&known_task_refs, &opts.only, "--only", &**log);
    filter::warn_unmatched_filters(&known_task_refs, &opts.skip, "--skip", &**log);
    let filtered: Vec<&dyn Task> = all_tasks
        .iter()
        .filter(|task| task_passes_filters(task.name(), &opts.only, &opts.skip))
        .map(Box::as_ref)
        .collect();

    if !opts.only.is_empty() || !opts.skip.is_empty() {
        let names: Vec<&str> = filtered.iter().map(|t| t.name()).collect();
        log.debug(&format!(
            "active filters — running {} task(s): {}",
            names.len(),
            names.join(", ")
        ));
    }

    runner.run_with_late_tasks(filtered, TaskPhase::Sync, || {
        runner
            .overlay_script_tasks()
            .into_iter()
            .filter(|task| task_passes_filters(task.name(), &opts.only, &opts.skip))
            .collect()
    })
}

fn task_passes_filters(task_name: &str, only: &[String], skip: &[String]) -> bool {
    let passes_only = only.is_empty()
        || only
            .iter()
            .any(|filter| filter::task_matches_filter(task_name, filter));
    let passes_skip = skip.is_empty()
        || !skip
            .iter()
            .any(|filter| filter::task_matches_filter(task_name, filter));
    passes_only && passes_skip
}

fn has_unmatched_filter(tasks: &[&dyn Task], filters: &[String]) -> bool {
    filters.iter().any(|filter| {
        !tasks
            .iter()
            .any(|task| filter::task_matches_filter(task.name(), filter))
    })
}

/// Resolve the dotfiles root directory from CLI arguments or auto-detection.
///
/// # Errors
///
/// Returns an error if the root directory cannot be determined or doesn't exist.
pub fn resolve_root(global: &GlobalOpts) -> Result<std::path::PathBuf> {
    // current_dir() is only needed as a last resort; obtain it lazily so that
    // failures (e.g. deleted cwd) don't block the faster lookup paths.
    let cwd = std::env::current_dir().ok();
    resolve_root_from_dir(global, cwd.as_deref())
}

/// Inner implementation of [`resolve_root`] that accepts an optional current
/// directory, making it testable without mutating process-global state.
///
/// Pass `Some(path)` to use an explicit directory; pass `None` to skip the
/// current-directory fallback (the other lookup strategies still apply).
fn resolve_root_from_dir(
    global: &GlobalOpts,
    cwd: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    if let Some(ref root) = global.root {
        return crate::infra::fs::canonicalize(root);
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
                return crate::infra::fs::canonicalize(candidate);
            }
        }
    }

    // Last resort: provided current directory
    if let Some(cwd) = cwd
        && cwd.join("conf").exists()
        && cwd.join("symlinks").exists()
    {
        return crate::infra::fs::canonicalize(cwd);
    }

    anyhow::bail!("cannot determine dotfiles root. Use --root or set DOTFILES_ROOT env var");
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    #[test]
    fn install_mode_excludes_update_phase() {
        assert!(!RunMode::Install.includes_phase(TaskPhase::Update));
        assert!(RunMode::Install.includes_phase(TaskPhase::Provision));
        assert!(RunMode::Update.includes_phase(TaskPhase::Update));
    }

    #[test]
    fn resolve_root_uses_explicit_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let global = GlobalOpts {
            root: Some(temp_dir.path().to_path_buf()),
            profile: None,
            dry_run: false,
            overlay: None,
            parallel: true,
        };

        let result = resolve_root(&global);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            crate::infra::fs::canonicalize(temp_dir.path()).unwrap()
        );
    }

    #[test]
    fn resolve_root_canonicalizes_explicit_relative_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let relative_root = temp_dir.path().join(".");
        let global = GlobalOpts {
            root: Some(relative_root),
            profile: None,
            dry_run: false,
            overlay: None,
            parallel: true,
        };

        let result = resolve_root(&global);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            crate::infra::fs::canonicalize(temp_dir.path()).unwrap()
        );
    }

    #[test]
    fn resolve_root_error_when_not_in_repo() {
        // Use a temp dir that definitely doesn't have conf/symlinks
        let temp_dir = tempfile::tempdir().unwrap();

        let global = GlobalOpts {
            root: None,
            profile: None,
            dry_run: false,
            overlay: None,
            parallel: true,
        };

        // Call the inner function directly — no process-global mutation needed.
        // Only check error if DOTFILES_ROOT env var is not set
        if std::env::var("DOTFILES_ROOT").is_err() {
            let result = resolve_root_from_dir(&global, Some(temp_dir.path()));
            assert!(result.is_err());
            if let Err(e) = result {
                assert!(e.to_string().contains("cannot determine dotfiles root"));
            }
        }
    }

    fn sample_install_tasks() -> Vec<Box<dyn Task>> {
        use crate::app::catalog::all_install_tasks;
        use crate::app::config::store::ConfigStore;
        use crate::test_helpers::empty_config;
        let config = empty_config(std::path::PathBuf::from("/tmp"));
        all_install_tasks(ConfigStore::from_config(config))
    }

    // ------------------------------------------------------------------
    // warn_unmatched_filters
    // ------------------------------------------------------------------

    #[test]
    fn warn_unmatched_filters_warns_on_no_match() {
        use crate::infra::logging::Logger;
        let log = Logger::new("test");
        let all = sample_install_tasks();
        let task_refs: Vec<&dyn Task> = all.iter().map(Box::as_ref).collect();

        // "xyznonexistent" should not match any task
        filter::warn_unmatched_filters(&task_refs, &["xyznonexistent".to_string()], "--only", &log);
        // Verification: the function runs without panic; the warning is
        // emitted via log.warn() which is captured by the Logger.
    }

    #[test]
    fn warn_unmatched_filters_silent_on_valid_match() {
        use crate::infra::logging::Logger;
        let log = Logger::new("test");
        let all = sample_install_tasks();
        let task_refs: Vec<&dyn Task> = all.iter().map(Box::as_ref).collect();

        filter::warn_unmatched_filters(&task_refs, &["symlinks".to_string()], "--only", &log);
    }
}
