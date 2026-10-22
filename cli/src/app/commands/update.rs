//! Update command implementation.
use anyhow::Result;
use std::sync::Arc;

use super::RuntimePolicy;
use crate::app::cli::UpdateOpts;
use crate::app::filter::{apply_task_filters, task_matches_filter};
use crate::domains::ai::apm::ApmPackageMode;
use crate::domains::repository::update::{RepositoryUpdateSignal, UpdateRepository};
use crate::engine::{Task, TaskId};
use crate::infra::logging::Logger;
use crate::infra::logging::OutputExt as _;

/// Run the update command.
///
/// Converges the system to the declared state and advances eligible locked
/// dependency versions.
///
/// # Errors
///
/// Returns an error if profile resolution, configuration loading, or task execution fails.
pub fn run(
    runtime: &RuntimePolicy<'_>,
    opts: &UpdateOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
) -> Result<()> {
    super::record_scope(
        log,
        &opts.filters,
        opts.with_deps,
        runtime.global.no_repo_update,
    );
    let run_lock = super::prepare_self_update(runtime, log)?;
    let runner = super::CommandRunner::new_with_lock(runtime, log, token, run_lock)?;

    let repository_update = RepositoryUpdateSignal::new();
    let mut all_tasks = runner.update_tasks_for_run(&repository_update, ApmPackageMode::UpdatePins);
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
