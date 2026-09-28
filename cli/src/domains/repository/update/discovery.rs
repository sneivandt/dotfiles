//! Git-state discovery: readiness checks, dry-run status, and remote SHA probing.
use anyhow::Result;
use std::path::Path;

use crate::engine::{Context, TaskResult, TaskStats};
use crate::infra::exec::ExecError;

use super::git_command;
use super::models::{
    CheckedRepository, DryRunUpdateStatus, RepositoryReadiness, RepositorySetReadiness,
    UpdateTarget, UpdateTargetKind,
};
use crate::infra::logging::OutputExt as _;

fn optional_git_output(ctx: &Context, root: &Path, args: &[&str]) -> Result<Option<String>> {
    match ctx.executor().execute(git_command(ctx, root, args)) {
        Ok(result) => Ok(Some(result.stdout.trim().to_string())),
        Err(ExecError::NonZero { .. }) => Ok(None),
        Err(
            error @ (ExecError::Cancelled { .. }
            | ExecError::TimedOut { .. }
            | ExecError::Spawn { .. }
            | ExecError::Io { .. }),
        ) => Err(error.into()),
    }
}

/// Build the list of repositories to consider for update (main + optional overlay).
pub(super) fn update_targets(ctx: &Context) -> Vec<UpdateTarget> {
    let mut targets = vec![UpdateTarget::new(
        UpdateTargetKind::Main,
        ctx.root().to_path_buf(),
    )];

    if let Some(overlay) = ctx.overlay()
        && overlay.join(".git").exists()
    {
        targets.push(UpdateTarget::new(
            UpdateTargetKind::Overlay,
            overlay.to_path_buf(),
        ));
    }

    targets
}

/// Check every update target and return the set that is ready to pull, or the
/// first blocked or inapplicable reason encountered.
pub(super) fn checked_repositories(ctx: &Context) -> Result<RepositorySetReadiness> {
    let targets = update_targets(ctx);
    let mut repositories = Vec::with_capacity(targets.len());
    for target in targets {
        match check_repository_ready(ctx, target)? {
            RepositoryReadiness::Ready(repository) => repositories.push(repository),
            RepositoryReadiness::Blocked(reason) => {
                return Ok(RepositorySetReadiness::Blocked(reason));
            }
            RepositoryReadiness::NotApplicable(reason) => {
                return Ok(RepositorySetReadiness::NotApplicable(reason));
            }
        }
    }
    Ok(RepositorySetReadiness::Ready(repositories))
}

/// Verify that a single repository is on a branch and has no tracked-file
/// changes, returning a [`CheckedRepository`] when safe to proceed.
pub(super) fn check_repository_ready(
    ctx: &Context,
    target: UpdateTarget,
) -> Result<RepositoryReadiness> {
    // Skip when not on a branch (e.g. detached HEAD in CI checkouts).
    let Some(head_ref) =
        optional_git_output(ctx, &target.root, &["symbolic-ref", "--quiet", "HEAD"])?
    else {
        let reason = target.reason("detached HEAD");
        ctx.log().info(format!("{reason}, skipping pull"));
        return Ok(RepositoryReadiness::NotApplicable(reason));
    };

    // Refuse to pull when tracked files are dirty. Untracked files do not
    // block a fast-forward pull, so they should not prevent updates.
    if worktree_has_local_changes(ctx, &target.root)? {
        return Ok(RepositoryReadiness::Blocked(
            target.reason("local changes present"),
        ));
    }

    Ok(RepositoryReadiness::Ready(CheckedRepository {
        target,
        head_ref,
    }))
}

/// Produce a dry-run result by comparing HEAD against the known upstream SHA
/// without making any mutations.
pub(super) fn dry_run_repositories(
    ctx: &Context,
    repositories: &[CheckedRepository],
) -> Result<TaskResult> {
    let mut would_update = false;
    for repository in repositories {
        match dry_run_update_status(ctx, &repository.target.root, &repository.head_ref)? {
            DryRunUpdateStatus::AlreadyCurrent => {
                ctx.log().debug(format!(
                    "{} already up to date",
                    repository.target.description()
                ));
            }
            DryRunUpdateStatus::WouldUpdate | DryRunUpdateStatus::Unknown => {
                ctx.log().dry_run(repository.target.dry_run_action());
                would_update = true;
            }
        }
    }

    Ok(if would_update {
        TaskStats::changed().finish()
    } else {
        TaskResult::Ok
    })
}

/// Determine whether a pull would change HEAD by comparing it against the
/// upstream SHA without fetching.
pub(super) fn dry_run_update_status(
    ctx: &Context,
    root: &Path,
    head_ref: &str,
) -> Result<DryRunUpdateStatus> {
    let head = ctx
        .executor()
        .execute(git_command(ctx, root, &["rev-parse", "HEAD"]))?;
    let upstream = match upstream_remote_sha(ctx, root, head_ref)? {
        Some(sha) => Some(sha),
        None => optional_git_output(ctx, root, &["rev-parse", "@{u}"])?,
    };

    Ok(match upstream {
        Some(sha) if head.stdout.trim() == sha => DryRunUpdateStatus::AlreadyCurrent,
        Some(_) => DryRunUpdateStatus::WouldUpdate,
        None => DryRunUpdateStatus::Unknown,
    })
}

/// Query the remote via `ls-remote` to get the SHA of the upstream branch
/// without relying on cached `FETCH_HEAD`.
pub(super) fn upstream_remote_sha(
    ctx: &Context,
    root: &Path,
    head_ref: &str,
) -> Result<Option<String>> {
    let branch = head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref);
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");

    let Some(remote) = optional_git_output(ctx, root, &["config", "--get", &remote_key])? else {
        return Ok(None);
    };
    let Some(merge_ref) = optional_git_output(ctx, root, &["config", "--get", &merge_key])? else {
        return Ok(None);
    };

    if remote.is_empty() || merge_ref.is_empty() {
        return Ok(None);
    }

    let Some(ls_remote) = optional_git_output(
        ctx,
        root,
        &["ls-remote", "--exit-code", &remote, &merge_ref],
    )?
    else {
        return Ok(None);
    };

    Ok(ls_remote.split_whitespace().next().map(ToString::to_string))
}

/// Return `true` when tracked files in the worktree have uncommitted
/// modifications. Untracked files are intentionally ignored.
pub(super) fn worktree_has_local_changes(ctx: &Context, root: &Path) -> Result<bool> {
    let status = ctx.executor().execute(git_command(
        ctx,
        root,
        &["status", "--porcelain", "--untracked-files=no"],
    ))?;

    Ok(!status.stdout.trim().is_empty())
}
