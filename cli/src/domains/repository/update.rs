//! Task: update the dotfiles repository.
//!
//! Git-state discovery, update planning, and mutation live in focused child
//! modules. This file owns task metadata and the operation lifecycle.
use anyhow::Result;
use std::path::Path;

use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskResultDisplay, process_operation,
};
use crate::infra::exec::CommandSpec;

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
    result_display: TaskResultDisplay,
}

impl UpdateRepository {
    /// Create a new task sharing the application's restart signal.
    #[must_use]
    pub const fn new(repo_updated: RepositoryUpdateSignal) -> Self {
        Self {
            repo_updated,
            result_display: TaskResultDisplay::RestartNotice,
        }
    }

    pub(crate) fn standalone() -> Self {
        Self {
            repo_updated: RepositoryUpdateSignal::new(),
            result_display: TaskResultDisplay::Standard,
        }
    }
}

impl Task for UpdateRepository {
    fn meta(&self) -> crate::engine::TaskMeta<'_> {
        crate::engine::TaskMeta::new("Dotfiles repository")
            .with_selector("repository")
            .with_result_display(self.result_display)
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(
            ctx,
            &UpdateRepositoryOperation {
                repo_updated: &self.repo_updated,
            },
        )
    }
}

#[derive(Debug)]
struct UpdateRepositoryOperation<'a> {
    repo_updated: &'a RepositoryUpdateSignal,
}

fn git_command(ctx: &Context, root: &Path, args: &[&str]) -> CommandSpec {
    // Use the repository-local names from `git rev-parse --local-env-vars`.
    // Git hooks can export these for their own repository. Keep global and
    // system config files and transport variables such as GIT_SSH_COMMAND.
    const LOCAL_ENV: &[&str] = &[
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ];
    CommandSpec::new("git")
        .args(args)
        .current_dir(root)
        .env("HOME", ctx.home().as_os_str())
        .remove_inherited_env(LOCAL_ENV)
}

impl Operation for UpdateRepositoryOperation<'_> {
    type Plan = Vec<CheckedRepository>;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        match checked_repositories(ctx)? {
            RepositorySetReadiness::Ready(repositories) if repositories.is_empty() => {
                Ok(OperationState::Complete)
            }
            RepositorySetReadiness::Ready(repositories) => {
                Ok(OperationState::needs_run(repositories))
            }
            RepositorySetReadiness::Blocked(reason) => Ok(OperationState::blocked(reason)),
            RepositorySetReadiness::NotApplicable(reason) => {
                Ok(OperationState::not_applicable(reason))
            }
        }
    }

    fn preview(&self, ctx: &Context, repositories: &Self::Plan) -> Result<TaskResult> {
        dry_run_repositories(ctx, repositories)
    }

    fn apply(&self, ctx: &Context, repositories: &Self::Plan) -> Result<TaskResult> {
        apply_repository_updates(ctx, repositories, self.repo_updated)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code uses panicking helpers"
)]
mod tests;
