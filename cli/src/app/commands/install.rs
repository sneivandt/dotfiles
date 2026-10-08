//! Update command implementation.
use anyhow::Result;
use std::sync::Arc;

use super::RuntimePolicy;
use crate::app::cli::InstallOpts;
use crate::app::filter::{apply_task_filters, task_matches_filter};
use crate::domains::ai::apm::ApmPackageMode;
use crate::domains::repository::update::{RepositoryUpdateSignal, UpdateRepository};
use crate::engine::{Task, TaskId};
use crate::infra::logging::Logger;
use crate::infra::logging::OutputExt as _;

/// The same update-only membership rule applies to execution and discovery.
pub(super) fn includes_task(task: &dyn Task, update_pins: bool) -> bool {
    update_pins || !task.update_only()
}

/// Run the update command.
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
    let apm_mode = if update_pins {
        ApmPackageMode::UpdatePins
    } else {
        ApmPackageMode::Install
    };
    let run_lock = super::prepare_self_update(runtime, log)?;
    let runner = super::CommandRunner::new_with_lock(runtime, log, token, run_lock)?;

    let repository_update = RepositoryUpdateSignal::new();
    let mut all_tasks = runner.install_tasks_for_run(&repository_update, apm_mode);

    // Version-advancing tasks are included in every update run. Filter
    // membership before user filters so warnings reflect eligible tasks.
    all_tasks.retain(|task| includes_task(task.as_ref(), update_pins));
    let repository_task = TaskId::Type(std::any::TypeId::of::<UpdateRepository>());
    let mut effective_skip = opts.filters.skip.clone();
    if runtime.global.no_repo_update {
        let repository = all_tasks
            .iter()
            .find(|task| task.task_id() == repository_task)
            .map(Box::as_ref);
        reject_disabled_repository_selection(repository, &opts.filters.only)?;
        if let Some(repository) = repository {
            effective_skip.retain(|selector| !task_matches_filter(repository, selector));
        }
        all_tasks.retain(|task| task.task_id() != repository_task);
        log.debug("repository update disabled — using the current checkout");
    }

    let startup_overlay_tasks = runner.overlay_script_tasks();
    let mut filtered = apply_task_filters(
        &all_tasks,
        &startup_overlay_tasks,
        &opts.filters.only,
        &effective_skip,
        opts.with_deps,
        log,
    )?;

    if runtime.repository_child {
        filtered.retain(|task| task.task_id() != repository_task);
    }

    runner.run_with_restart(
        filtered,
        repository_task,
        move || runtime.restart_after_repository_update(repository_update.was_updated()),
        || super::re_exec_after_repository_update(&**log),
    )
}

fn reject_disabled_repository_selection(
    repository: Option<&dyn Task>,
    only: &[String],
) -> Result<()> {
    if repository.is_some_and(|task| {
        only.iter()
            .any(|selector| task_matches_filter(task, selector))
    }) {
        anyhow::bail!("--only cannot select 'repository' when --no-repo-update is set");
    }
    Ok(())
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

        assert!(!includes_task(&UpdateOnly, false));
        assert!(includes_task(&UpdateOnly, true));
    }

    #[test]
    fn no_repository_update_rejects_selecting_the_repository_task() {
        let repository = UpdateRepository::new(RepositoryUpdateSignal::new());
        let error =
            reject_disabled_repository_selection(Some(&repository), &["repository".to_string()])
                .expect_err("disabled repository task should not look like an unknown selector");
        assert_eq!(
            error.to_string(),
            "--only cannot select 'repository' when --no-repo-update is set"
        );
    }
}
