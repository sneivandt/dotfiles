//! Task: update the dotfiles repository.
//!
//! Git-state discovery, update planning, and mutation live in focused child
//! modules. This file owns task metadata and the operation lifecycle.
use anyhow::Result;

use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, process_operation, task_metadata,
};

mod apply;
mod discovery;
mod models;

use apply::apply_repository_updates;
use discovery::{checked_repositories, dry_run_repositories};
use models::{CheckedRepository, RepositorySetReadiness};

#[cfg(test)]
use self::discovery::worktree_has_local_changes;

/// Shared indication that the checkout changed and the command must restart.
#[derive(Debug, Clone, Default)]
pub struct RepositoryUpdateSignal {
    flag: crate::infra::atomic_flag::AtomicFlag,
}

impl RepositoryUpdateSignal {
    /// Create an unset repository update signal.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn mark_updated(&self) {
        self.flag.set();
    }

    /// Return whether a repository was updated during this command.
    #[must_use]
    pub(crate) fn was_updated(&self) -> bool {
        self.flag.get()
    }
}

/// Pull latest changes from the remote repository.
#[derive(Debug)]
pub struct UpdateRepository {
    /// Set when the repository changes so the application can restart from the
    /// freshly loaded checkout.
    pub(super) repo_updated: RepositoryUpdateSignal,
}

impl UpdateRepository {
    /// Create a new task sharing the application's restart signal.
    #[must_use]
    pub const fn new(repo_updated: RepositoryUpdateSignal) -> Self {
        Self { repo_updated }
    }
}

impl Task for UpdateRepository {
    task_metadata! {
        name: "Dotfiles repository",
        selector: "repository",
        deps: [crate::domains::repository::sparse_checkout::ConfigureSparseCheckout],
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
    repo_updated: RepositoryUpdateSignal,
}

impl UpdateRepositoryOperation {
    const fn new(repo_updated: RepositoryUpdateSignal) -> Self {
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
            RepositorySetReadiness::Blocked(reason) => Ok(OperationState::blocked(reason)),
            RepositorySetReadiness::NotApplicable(reason) => {
                Ok(OperationState::not_applicable(reason))
            }
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
    clippy::panic,
    clippy::significant_drop_tightening,
    reason = "test code uses panicking helpers"
)]
mod tests;
