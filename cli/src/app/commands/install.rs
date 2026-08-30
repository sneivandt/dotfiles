//! Install command implementation.
use anyhow::Result;
use std::sync::Arc;

use crate::app::cli::{GlobalOpts, InstallOpts};
use crate::app::filter::{apply_task_filters, task_passes_filters};
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
    global: &GlobalOpts,
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
    run_pipeline(global, opts, log, token, mode)
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
    global: &GlobalOpts,
    opts: &InstallOpts,
    log: &Arc<Logger>,
    token: &crate::engine::CancellationToken,
    mode: RunMode,
) -> Result<()> {
    let run_lock = super::prepare_self_update(global, log)?;
    let runner = super::CommandRunner::new_with_lock(global, log, token, run_lock)?;

    let repository_child = super::repository_reexec_active(runner.env());
    let repository_update = RepositoryUpdateSignal::new();
    let mut all_tasks = runner.install_tasks_for_run(&repository_update);

    // Version-advancing tasks are scheduled only with `--update-pins`. Filter
    // membership before user filters so warnings reflect eligible tasks.
    all_tasks.retain(|task| mode.includes_task(task.as_ref()));
    if global.no_repo_update {
        let repository_task = TaskId::Type(std::any::TypeId::of::<UpdateRepository>());
        all_tasks.retain(|task| task.task_id() != repository_task);
        log.debug("repository update disabled — using the current checkout");
    }

    let startup_overlay_tasks = runner.overlay_script_tasks();
    let boundary = TaskId::Type(std::any::TypeId::of::<UpdateRepository>());
    let recovery_selectors = runner.recovery_selectors().cloned();
    let mut filtered = if let Some(selectors) = recovery_selectors.as_ref() {
        if !opts.only.is_empty() || !opts.skip.is_empty() {
            anyhow::bail!("--retry-failed cannot be combined with --only or --skip");
        }
        let dynamic_retry = startup_overlay_tasks
            .iter()
            .any(|task| crate::app::recovery::task_selected(task.as_ref(), selectors));
        let required = if dynamic_retry {
            std::slice::from_ref(&boundary)
        } else {
            &[]
        };
        crate::app::recovery::select_tasks(&all_tasks, &startup_overlay_tasks, selectors, required)?
    } else {
        apply_task_filters(
            &all_tasks,
            &startup_overlay_tasks,
            &opts.only,
            &opts.skip,
            opts.with_deps,
            log,
        )?
    };

    omit_repository_task(&mut filtered, repository_child);
    filtered.extend(
        startup_overlay_tasks
            .iter()
            .filter(|task| {
                recovery_selectors.as_ref().map_or_else(
                    || task_passes_filters(task.as_ref(), &opts.only, &opts.skip),
                    |selectors| crate::app::recovery::task_selected(task.as_ref(), selectors),
                )
            })
            .map(Box::as_ref),
    );

    runner.run_with_restart(
        filtered,
        boundary,
        move || repository_update.was_updated() && !crate::infra::elevation::is_elevated_child(),
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

    #[test]
    fn repository_retry_selector_is_resolved_before_child_omits_update_task() {
        let tasks = sample_install_tasks();
        let selectors = std::collections::HashSet::from(["repository".to_string()]);
        let mut selected =
            crate::app::recovery::select_tasks(&tasks, &[], &selectors, &[]).unwrap();

        omit_repository_task(&mut selected, true);

        assert!(
            selected
                .iter()
                .all(|task| task.task_id()
                    != TaskId::Type(std::any::TypeId::of::<UpdateRepository>())),
            "the guarded child must not synchronize the repository again"
        );
        assert!(
            selected.is_empty(),
            "the child already completed repository recovery"
        );
    }

    fn sample_install_tasks() -> Vec<Box<dyn Task>> {
        use crate::app::catalog::all_install_tasks;
        use crate::app::config::store::ConfigStore;
        use crate::test_helpers::empty_config;
        let config = empty_config(std::path::PathBuf::from("/tmp"));
        all_install_tasks(&ConfigStore::from_config(config))
    }
}
