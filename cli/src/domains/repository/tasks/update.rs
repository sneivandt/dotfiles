//! Task: update the dotfiles repository.
//!
//! Git-state discovery, update planning, and mutation live in focused child
//! modules. This file owns task metadata and the operation lifecycle.
use anyhow::Result;

use crate::engine::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, UpdateSignal,
    process_operation, task_metadata,
};

mod apply;
mod discovery;
mod models;

use apply::apply_repository_updates;
use discovery::{checked_repositories, dry_run_repositories};
use models::{CheckedRepository, RepositorySetReadiness};

#[cfg(test)]
use self::discovery::worktree_has_local_changes;

/// Pull latest changes from the remote repository.
#[derive(Debug)]
pub struct UpdateRepository {
    /// Set to `true` when the repository is actually updated by this task.
    ///
    /// Shared with [`super::reload_config::ReloadConfig`] so that the task can
    /// skip the reload when the repository was already up to date.
    pub(super) repo_updated: UpdateSignal,
}

impl UpdateRepository {
    /// Create a new task, sharing `repo_updated` with `ReloadConfig`.
    #[must_use]
    pub const fn new(repo_updated: UpdateSignal) -> Self {
        Self { repo_updated }
    }
}

impl Task for UpdateRepository {
    task_metadata! {
        name: "Update repository",
        phase: TaskPhase::Sync,
        domain: Domain::Repository,
        deps: [crate::domains::repository::tasks::sparse_checkout::ConfigureSparseCheckout],
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(
            ctx,
            &UpdateRepositoryOperation::new(self.repo_updated.clone()),
        )
    }
}

#[derive(Debug)]
struct UpdateRepositoryOperation {
    repo_updated: UpdateSignal,
}

impl UpdateRepositoryOperation {
    const fn new(repo_updated: UpdateSignal) -> Self {
        Self { repo_updated }
    }
}

impl Operation for UpdateRepositoryOperation {
    type Plan = Vec<CheckedRepository>;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let home_str = ctx.home().to_string_lossy().into_owned();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];
        match checked_repositories(ctx, git_env)? {
            RepositorySetReadiness::Ready(repositories) if repositories.is_empty() => {
                Ok(OperationState::Complete)
            }
            RepositorySetReadiness::Ready(repositories) => Ok(OperationState::needs_run(
                "update repositories",
                repositories,
            )),
            RepositorySetReadiness::Skipped(reason) => Ok(OperationState::blocked(reason)),
        }
    }

    fn preview(&self, ctx: &Context, repositories: &Self::Plan) -> Result<TaskResult> {
        let home_str = ctx.home().to_string_lossy().into_owned();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];
        dry_run_repositories(ctx, repositories, git_env)
    }

    fn apply(&self, ctx: &Context, repositories: &Self::Plan) -> Result<TaskResult> {
        let home_str = ctx.home().to_string_lossy().into_owned();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];
        apply_repository_updates(ctx, repositories, git_env, &self.repo_updated)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
