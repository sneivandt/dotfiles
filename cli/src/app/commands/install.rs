//! Install command implementation.
use anyhow::Result;
use std::sync::Arc;

use super::RuntimePolicy;
use crate::app::cli::InstallOpts;
use crate::app::filter::apply_task_filters;
use crate::domains::repository::update::{RepositoryUpdateSignal, UpdateRepository};
use crate::engine::{Task, TaskId};
use crate::infra::logging::Logger;
use crate::infra::logging::OutputExt as _;

/// Install pipeline behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunMode {
    /// Converge to declared state without advancing locked versions.
    Install,
    /// Converge and advance locked dependency versions.
    Update,
}

impl RunMode {
    fn includes_task(self, task: &dyn Task) -> bool {
        matches!(self, Self::Update) || !task.update_only()
    }
}

/// Run the install command.
///
/// Converges the system to the declared state and optionally advances locked
/// dependency versions when `update_pins` is set.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    runtime: &RuntimePolicy<'_>,
    opts: &InstallOpts,
    update_pins: bool,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    let mode = if update_pins {
        RunMode::Update
    } else {
        RunMode::Install
    };
    run_pipeline(runtime, opts, log, token, mode)
}

/// Shared implementation for normal installation and optional pin updates.
///
/// The two commands run the identical task graph; `mode` determines whether
/// version-advancing tasks additionally move locked refs forward.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub(crate) fn run_pipeline(
    runtime: &RuntimePolicy<'_>,
    opts: &InstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
    mode: RunMode,
) -> Result<()> {
    let run_lock = super::prepare_self_update(runtime, log)?;
    let runner = super::CommandRunner::new_with_lock(runtime, log, token, run_lock)?;

    let repository_update = RepositoryUpdateSignal::new();
    let mut all_tasks = runner.install_tasks_for_run(&repository_update);

    // Version-advancing tasks are scheduled only with `--update-pins`. Filter
    // membership before user filters so warnings reflect eligible tasks.
    all_tasks.retain(|task| mode.includes_task(task.as_ref()));
    if runtime.global.no_repo_update {
        let repository_task = TaskId::Type(std::any::TypeId::of::<UpdateRepository>());
        all_tasks.retain(|task| task.task_id() != repository_task);
        log.debug("repository update disabled — using the current checkout");
    }

    let startup_overlay_tasks = runner.overlay_script_tasks();
    let boundary = TaskId::Type(std::any::TypeId::of::<UpdateRepository>());
    let mut filtered = apply_task_filters(
        &all_tasks,
        &startup_overlay_tasks,
        &opts.only,
        &opts.skip,
        opts.with_deps,
        log,
    )?;

    omit_repository_task(&mut filtered, runtime.repository_child);

    runner.run_with_restart(
        filtered,
        boundary,
        move || runtime.restart_after_repository_update(repository_update.was_updated()),
        || super::re_exec_after_repository_update(&**log),
    )
}

fn omit_repository_task(tasks: &mut Vec<&dyn Task>, repository_child: bool) {
    if repository_child {
        let repository_task = TaskId::Type(std::any::TypeId::of::<UpdateRepository>());
        tasks.retain(|task| task.task_id() != repository_task);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TaskMeta;

    #[test]
    fn install_mode_excludes_update_only_tasks() {
        #[derive(Debug)]
        struct UpdateOnly;
        impl Task for UpdateOnly {
            fn meta(&self) -> TaskMeta<'_> {
                TaskMeta::new("update only").with_update_only(true)
            }

            fn run(&self, _ctx: &crate::engine::Context) -> Result<crate::engine::TaskResult> {
                Ok(crate::engine::TaskResult::Ok)
            }
        }

        assert!(!RunMode::Install.includes_task(&UpdateOnly));
        assert!(RunMode::Update.includes_task(&UpdateOnly));
    }
}
