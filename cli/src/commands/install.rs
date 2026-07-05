//! Install command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::cli::{GlobalOpts, InstallOpts};
use crate::logging::Logger;
use crate::tasks;

/// Run the install command.
///
/// Converges the system to the declared state without advancing locked
/// dependency versions (see [`crate::commands::update`] for that).
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
    run_pipeline(global, opts, log, token, false)
}

/// Shared implementation behind both `install` and `update`.
///
/// The two commands run the identical task graph; `advance_versions`
/// distinguishes them.  When `false` (install) the pipeline converges to the
/// declared state.  When `true` (update) version-advancing tasks — currently
/// the APM dependency refresh — additionally move locked refs forward.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub(crate) fn run_pipeline(
    global: &GlobalOpts,
    opts: &InstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
    advance_versions: bool,
) -> Result<()> {
    super::prepare_self_update(global, log)?;

    let runner =
        super::CommandRunner::new(global, log, token)?.with_advance_versions(advance_versions);

    // Build the full task list: static tasks + dynamic overlay script tasks.
    let mut all_tasks = tasks::all_install_tasks();
    all_tasks.extend(runner.overlay_script_tasks());

    // Version-advancing tasks (the `Update` phase) are only scheduled by the
    // `update` command.  Drop them here for `install` so the `Updating
    // dependencies` phase is empty (its header is suppressed) and so `--only`/
    // `--skip` warnings and matching below reason about the command-eligible
    // task set rather than tasks that could never run.
    if !advance_versions {
        all_tasks.retain(|t| t.phase() != tasks::TaskPhase::Update);
    }

    if !log.is_verbose()
        && (has_unmatched_filter(&all_tasks, &opts.only)
            || has_unmatched_filter(&all_tasks, &opts.skip))
    {
        log.separate_from_startup();
    }
    tasks::filter::warn_unmatched_filters(&all_tasks, &opts.only, "--only", &**log);
    tasks::filter::warn_unmatched_filters(&all_tasks, &opts.skip, "--skip", &**log);
    let filtered: Vec<&dyn tasks::Task> = all_tasks
        .iter()
        .filter(|t| {
            // Both --only and --skip can be active simultaneously.
            // A task runs if it matches an --only filter (or no --only was given)
            // AND it doesn't match any --skip filter.
            let passes_only = opts.only.is_empty()
                || opts
                    .only
                    .iter()
                    .any(|filter| tasks::filter::task_matches_filter(t.name(), filter));
            let passes_skip = opts.skip.is_empty()
                || !opts
                    .skip
                    .iter()
                    .any(|filter| tasks::filter::task_matches_filter(t.name(), filter));
            passes_only && passes_skip
        })
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

    runner.run(filtered)
}

fn has_unmatched_filter(tasks: &[Box<dyn tasks::Task>], filters: &[String]) -> bool {
    filters.iter().any(|filter| {
        !tasks
            .iter()
            .any(|task| tasks::filter::task_matches_filter(task.name(), filter))
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
        return crate::fs::canonicalize(root);
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
                return crate::fs::canonicalize(candidate);
            }
        }
    }

    // Last resort: provided current directory
    if let Some(cwd) = cwd
        && cwd.join("conf").exists()
        && cwd.join("symlinks").exists()
    {
        return crate::fs::canonicalize(cwd);
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
            crate::fs::canonicalize(temp_dir.path()).unwrap()
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
            crate::fs::canonicalize(temp_dir.path()).unwrap()
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

    // ------------------------------------------------------------------
    // warn_unmatched_filters
    // ------------------------------------------------------------------

    #[test]
    fn warn_unmatched_filters_warns_on_no_match() {
        use crate::logging::Logger;
        let log = Logger::new("test");
        let all = tasks::all_install_tasks();

        // "xyznonexistent" should not match any task
        tasks::filter::warn_unmatched_filters(
            &all,
            &["xyznonexistent".to_string()],
            "--only",
            &log,
        );
        // Verification: the function runs without panic; the warning is
        // emitted via log.warn() which is captured by the Logger.
    }

    #[test]
    fn warn_unmatched_filters_silent_on_valid_match() {
        use crate::logging::Logger;
        let log = Logger::new("test");
        let all = tasks::all_install_tasks();

        tasks::filter::warn_unmatched_filters(&all, &["symlinks".to_string()], "--only", &log);
    }
}
