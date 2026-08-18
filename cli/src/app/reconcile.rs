//! Reconcile profile-sensitive checkout state after configuration reload.

use anyhow::Result;

use crate::app::config::store::ConfigStore;
use crate::app::preserve::MaterializeExcludedSymlinks;
use crate::domains::repository::sparse_checkout::ConfigureSparseCheckout;
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, UpdateSignal, process_operation,
    task_metadata,
};

/// Reapply preservation and sparse-checkout state from freshly reloaded config.
#[derive(Debug)]
pub struct ReconcileUpdatedCheckout {
    repo_updated: UpdateSignal,
    store: ConfigStore,
}

impl ReconcileUpdatedCheckout {
    /// Create a reconciliation task sharing the update signal and config store.
    #[must_use]
    pub const fn new(repo_updated: UpdateSignal, store: ConfigStore) -> Self {
        Self {
            repo_updated,
            store,
        }
    }

    fn reconcile(&self, ctx: &Context) -> Result<TaskResult> {
        let preserve = MaterializeExcludedSymlinks::new(
            self.store.all_symlinks.clone(),
            self.store.manifest.clone(),
        );
        if preserve.should_run(ctx) {
            let result = preserve.run(ctx)?;
            if result_failed(&result) {
                return Ok(result);
            }
        }

        let sparse = ConfigureSparseCheckout::new(self.store.manifest.clone());
        if sparse.should_run(ctx) {
            let result = sparse.run(ctx)?;
            if let TaskResult::Skipped { reason, .. } = result {
                // Repository updates require a clean tracked worktree before
                // setting the signal. A skip here therefore means the checkout
                // changed between the update and reconciliation, and continuing
                // would expose downstream tasks to mismatched config and files.
                return Ok(TaskResult::Failed(format!(
                    "post-update sparse checkout was skipped: {reason}"
                )));
            }
            if result_failed(&result) {
                return Ok(result);
            }
        }

        Ok(TaskResult::Ok)
    }
}

const fn result_failed(result: &TaskResult) -> bool {
    matches!(result, TaskResult::Failed(_))
        || matches!(result, TaskResult::Batch(stats) if stats.failed_count() > 0)
}

struct ReconcileUpdatedCheckoutOperation<'a> {
    task: &'a ReconcileUpdatedCheckout,
}

impl Operation for ReconcileUpdatedCheckoutOperation<'_> {
    type Plan = ();

    fn current_state(&self, _ctx: &Context) -> Result<OperationState<Self::Plan>> {
        if self.task.repo_updated.was_updated() {
            Ok(OperationState::needs_run("configuration refreshed", ()))
        } else {
            Ok(OperationState::not_applicable(
                "repository was already current",
            ))
        }
    }

    fn preview(&self, _ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, _plan: &Self::Plan) -> Result<TaskResult> {
        self.task.reconcile(ctx)
    }
}

impl Task for ReconcileUpdatedCheckout {
    task_metadata! {
        name: "Reconcile updated checkout",
        visibility: crate::engine::TaskVisibility::Internal,
        deps: [crate::app::reload::ReloadConfig],
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(ctx, &ReconcileUpdatedCheckoutOperation { task: self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn run_is_not_applicable_without_a_repository_update() {
        let task = ReconcileUpdatedCheckout::new(
            UpdateSignal::new(),
            ConfigStore::from_config(empty_config(PathBuf::from("/tmp"))),
        );
        let ctx = make_linux_context(empty_config(PathBuf::from("/tmp")));

        assert!(matches!(
            task.run(&ctx).unwrap(),
            TaskResult::NotApplicable(reason) if reason == "repository was already current"
        ));
    }
}
