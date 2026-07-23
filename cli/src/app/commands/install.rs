//! Install command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, InstallOpts};
use crate::app::filter::{self, has_unmatched_filter, task_passes_filters};
use crate::engine::{Task, TaskId};
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

    fn includes_task(self, task: &dyn Task) -> bool {
        self.advances_versions() || !task.update_only()
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
    super::prepare_self_update(global, &**log)?;

    let runner = super::CommandRunner::new(global, log, token)?.with_run_mode(mode);

    // Build the static task list. Dynamic overlay scripts are rebuilt after
    // configuration reload so they observe changes pulled in this run.
    let mut all_tasks = runner.install_tasks();

    // Version-advancing tasks are only scheduled by `update`. Filter command
    // membership before user filters so warnings reflect eligible tasks.
    all_tasks.retain(|task| mode.includes_task(task.as_ref()));

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

    runner.run_with_late_tasks(
        filtered,
        TaskId::Type(std::any::TypeId::of::<crate::app::reload::ReloadConfig>()),
        || {
            runner
                .overlay_script_tasks()
                .into_iter()
                .filter(|task| task_passes_filters(task.name(), &opts.only, &opts.skip))
                .collect()
        },
    )
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
    fn install_mode_excludes_update_only_tasks() {
        #[derive(Debug)]
        struct UpdateOnly;
        impl Task for UpdateOnly {
            fn name(&self) -> &'static str {
                "update only"
            }
            fn update_only(&self) -> bool {
                true
            }
            fn run(&self, _ctx: &crate::engine::Context) -> Result<crate::engine::TaskResult> {
                Ok(crate::engine::TaskResult::Ok)
            }
        }

        assert!(!RunMode::Install.includes_task(&UpdateOnly));
        assert!(RunMode::Update.includes_task(&UpdateOnly));
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
