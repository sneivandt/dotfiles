//! Update command implementation.
use anyhow::Result;
use std::sync::Arc;

use super::RuntimePolicy;
use crate::app::cli::UpdateOpts;
use crate::app::filter::{apply_task_filters, normalize_task_filter, resolve_filters};
use crate::domains::ai::apm::ApmPackageMode;
use crate::domains::repository::update::{RepositoryUpdateSignal, UpdateRepository};
use crate::engine::Task;
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
    let repository_update = RepositoryUpdateSignal::new();
    let repository_only = UpdateRepository::standalone();
    let repository_task = repository_only.task_id();
    let is_repository_selector =
        |selector: &str| normalize_task_filter(selector) == repository_only.selector();
    if runtime.global.no_repo_update {
        reject_disabled_repository_selection(
            opts.filters.only.iter().any(|s| is_repository_selector(s)),
        )?;
    }
    let run_lock = super::prepare_self_update(runtime, log)?;
    if !runtime.repository_child
        && !opts.filters.only.is_empty()
        && opts.filters.skip.is_empty()
        && opts
            .filters
            .only
            .iter()
            .all(|selector| is_repository_selector(selector))
    {
        return super::CommandRunner::run_repository(runtime, log, token, &repository_only);
    }

    let runner = super::CommandRunner::new_with_lock(runtime, log, token, run_lock)?;
    let all_tasks = runner.update_tasks_for_run(&repository_update, ApmPackageMode::UpdatePins);
    let mut effective_skip = opts.filters.skip.clone();
    let startup_overlay_tasks = runner.overlay_script_tasks();
    let known: Vec<&dyn Task> = all_tasks
        .iter()
        .chain(&startup_overlay_tasks)
        .map(Box::as_ref)
        .collect();
    if runtime.repository_child && !runtime.execution.elevated_child {
        let exclusions = runtime
            .env
            .var(super::reexec::REPOSITORY_EXCLUSIONS_VAR)
            .ok_or_else(|| {
                anyhow::anyhow!("repository restart is missing accepted exclusions; retry update")
            })?;
        effective_skip = serde_json::from_str::<Vec<String>>(&exclusions)?;
        effective_skip.retain(|selector| {
            known
                .iter()
                .any(|task| task.visibility().is_visible() && task.selector() == selector)
        });
    }
    let excluded_ids = resolve_filters(&known, &effective_skip, "--skip")?;
    if runtime.global.no_repo_update {
        let included_ids = resolve_filters(&known, &opts.filters.only, "--only")?;
        reject_disabled_repository_selection(included_ids.contains(&repository_task))?;
        effective_skip.push(repository_only.selector().to_string());
        log.debug("repository update disabled — using the current checkout");
    }
    let exclusions = serde_json::to_string(
        &known
            .iter()
            .filter(|task| excluded_ids.contains(&task.task_id()))
            .map(|task| task.selector())
            .collect::<Vec<_>>(),
    )?;
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

    let config = runner.config_handle();
    let config = config.get();
    runner.run_with_restart(
        filtered,
        repository_task,
        move || runtime.restart_after_repository_update(repository_update.was_updated()),
        || {
            runner.warn_missing_managed_sources();
            super::re_exec_after_repository_update(
                &config.root,
                &config.profile.name,
                config.overlay.as_deref(),
                &exclusions,
                log,
            )
        },
    )
}

fn reject_disabled_repository_selection(selected: bool) -> Result<()> {
    if selected {
        anyhow::bail!("--only cannot select 'repository' when --no-repo-update is set");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_repository_update_rejects_selecting_the_repository_task() {
        let error = reject_disabled_repository_selection(true)
            .expect_err("disabled repository task should not look like an unknown selector");
        assert_eq!(
            error.to_string(),
            "--only cannot select 'repository' when --no-repo-update is set"
        );
    }
}
