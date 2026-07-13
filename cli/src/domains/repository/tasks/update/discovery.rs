//! Git-state discovery: readiness checks, dry-run status, and remote SHA probing.
use anyhow::Result;
use std::path::Path;

use crate::engine::{Context, TaskResult};

use super::models::{
    CheckedRepository, DryRunUpdateStatus, RepositoryReadiness, RepositorySetReadiness,
    UpdateTarget, UpdateTargetKind,
};

/// Build the list of repositories to consider for update (main + optional overlay).
pub(super) fn update_targets(ctx: &Context) -> Vec<UpdateTarget> {
    let mut targets = vec![UpdateTarget::new(UpdateTargetKind::Main, ctx.root())];

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
/// first skip reason encountered.
pub(super) fn checked_repositories(
    ctx: &Context,
    git_env: &[(&str, &str)],
) -> Result<RepositorySetReadiness> {
    let targets = update_targets(ctx);
    let mut repositories = Vec::with_capacity(targets.len());
    for target in targets {
        match check_repository_ready(ctx, target, git_env)? {
            RepositoryReadiness::Ready(repository) => repositories.push(repository),
            RepositoryReadiness::Skipped(reason) => {
                return Ok(RepositorySetReadiness::Skipped(reason));
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
    git_env: &[(&str, &str)],
) -> Result<RepositoryReadiness> {
    // Skip when not on a branch (e.g. detached HEAD in CI checkouts).
    let head_ref = if let Ok(result) = ctx.executor.run_in_with_env(
        &target.root,
        "git",
        &["symbolic-ref", "--quiet", "HEAD"],
        git_env,
    ) {
        result.stdout.trim().to_string()
    } else {
        let reason = target.reason("detached HEAD");
        ctx.log.info(&format!("{reason}, skipping pull"));
        return Ok(RepositoryReadiness::Skipped(reason));
    };

    // Refuse to pull when tracked files are dirty. Untracked files do not
    // block a fast-forward pull, so they should not prevent updates.
    if worktree_has_local_changes(ctx, &target.root, git_env)? {
        return Ok(RepositoryReadiness::Skipped(
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
    git_env: &[(&str, &str)],
) -> Result<TaskResult> {
    let mut would_update = false;
    for repository in repositories {
        match dry_run_update_status(ctx, &repository.target.root, git_env, &repository.head_ref)? {
            DryRunUpdateStatus::AlreadyCurrent => {
                ctx.log.debug(&format!(
                    "{} already up to date",
                    repository.target.description()
                ));
            }
            DryRunUpdateStatus::WouldUpdate | DryRunUpdateStatus::Unknown => {
                ctx.log.dry_run(&repository.target.dry_run_action());
                would_update = true;
            }
        }
    }

    Ok(if would_update {
        TaskResult::DryRun
    } else {
        TaskResult::Ok
    })
}

/// Determine whether a pull would change HEAD by comparing it against the
/// upstream SHA without fetching.
pub(super) fn dry_run_update_status(
    ctx: &Context,
    root: &Path,
    git_env: &[(&str, &str)],
    head_ref: &str,
) -> Result<DryRunUpdateStatus> {
    let head = ctx
        .executor
        .run_in_with_env(root, "git", &["rev-parse", "HEAD"], git_env)?;
    let head_sha = head.stdout.trim().to_string();

    if let Some(remote_sha) = upstream_remote_sha(ctx, root, git_env, head_ref) {
        return Ok(if head_sha == remote_sha {
            DryRunUpdateStatus::AlreadyCurrent
        } else {
            DryRunUpdateStatus::WouldUpdate
        });
    }

    if let Ok(upstream) = ctx
        .executor
        .run_in_with_env(root, "git", &["rev-parse", "@{u}"], git_env)
    {
        return Ok(if head_sha == upstream.stdout.trim() {
            DryRunUpdateStatus::AlreadyCurrent
        } else {
            DryRunUpdateStatus::WouldUpdate
        });
    }

    Ok(DryRunUpdateStatus::Unknown)
}

/// Query the remote via `ls-remote` to get the SHA of the upstream branch
/// without relying on cached `FETCH_HEAD`.
pub(super) fn upstream_remote_sha(
    ctx: &Context,
    root: &Path,
    git_env: &[(&str, &str)],
    head_ref: &str,
) -> Option<String> {
    let branch = head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref);
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");

    let remote = ctx
        .executor
        .run_in_with_env(root, "git", &["config", "--get", &remote_key], git_env)
        .ok()?;
    let merge_ref = ctx
        .executor
        .run_in_with_env(root, "git", &["config", "--get", &merge_key], git_env)
        .ok()?;

    let remote_name = remote.stdout.trim();
    let merge_name = merge_ref.stdout.trim();
    if remote_name.is_empty() || merge_name.is_empty() {
        return None;
    }

    let ls_remote = ctx
        .executor
        .run_in_with_env(
            root,
            "git",
            &["ls-remote", "--exit-code", remote_name, merge_name],
            git_env,
        )
        .ok()?;

    ls_remote
        .stdout
        .split_whitespace()
        .next()
        .map(ToString::to_string)
}

/// Return `true` when tracked files in the worktree have uncommitted
/// modifications. Untracked files are intentionally ignored.
pub(super) fn worktree_has_local_changes(
    ctx: &Context,
    root: &Path,
    git_env: &[(&str, &str)],
) -> Result<bool> {
    let status = ctx.executor.run_in_with_env(
        root,
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
        git_env,
    )?;

    Ok(!status.stdout.trim().is_empty())
}
