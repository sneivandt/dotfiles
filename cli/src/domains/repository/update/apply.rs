//! Execution: fetch, divergence check, and fast-forward merge.
use std::time::Duration;

use anyhow::Result;

use crate::engine::{Context, TaskResult, TaskStats};
use crate::infra::exec::ExecError;

use super::discovery::{compare_commits, upstream, worktree_has_local_changes};
use super::models::{
    CheckedRepository, CommitRelationship, RepositoryPlanReadiness, RepositoryUpdatePlan,
};
use super::{RepositoryUpdateSignal, git_command};
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::{log_thread_name, set_log_thread_name};

const MAX_FETCH_ATTEMPTS: u32 = 3;

const TRANSIENT_FETCH_ERROR_MARKERS: &[&str] = &[
    "broken pipe",
    "connection closed by remote host",
    "connection refused",
    "connection reset by peer",
    "connection timed out",
    "could not resolve hostname",
    "early eof",
    "failed to connect to new control master",
    "mux_client_request_session",
    "network is unreachable",
    "operation timed out",
    "recv failure: connection was reset",
    "remote end hung up unexpectedly",
    "remote host closed the connection",
    "temporary failure in name resolution",
    "unexpected disconnect while reading sideband packet",
];

/// Run `git fetch` for every repository, then fast-forward merge those that
/// have upstream commits.
pub(super) fn apply_repository_updates(
    ctx: &Context,
    repositories: &[CheckedRepository],
    repo_updated: &RepositoryUpdateSignal,
) -> Result<TaskResult> {
    // Fetch first so divergence can be evaluated without invoking `git pull`,
    // which fails noisily when the local branch has diverged from upstream.
    let mut tracked = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let Some((remote, reference)) =
            upstream(ctx, &repository.target.root, &repository.head_ref)?
        else {
            return Ok(TaskResult::unmet(
                repository.target.reason("no upstream tracking branch"),
            ));
        };
        tracked.push((repository, remote, reference));
    }
    let mut original_heads = Vec::with_capacity(repositories.len());
    for repository in repositories {
        let Some(head) = checkout_head(ctx, repository)? else {
            return Ok(TaskResult::unmet(checkout_changed(repository)));
        };
        original_heads.push(head);
    }
    let commits = match fetch_all(ctx, &tracked)? {
        Ok(commits) => commits,
        Err(reason) => return Ok(TaskResult::Failed(reason)),
    };

    let mut plans = Vec::with_capacity(repositories.len());
    for ((repository, original_head), commit) in
        repositories.iter().zip(original_heads).zip(&commits)
    {
        match plan_repository_update(ctx, repository, original_head, commit)? {
            RepositoryPlanReadiness::Ready(plan) => plans.push(plan),
            RepositoryPlanReadiness::Skipped(reason) => return Ok(TaskResult::unmet(reason)),
        }
    }
    advance_worktrees(ctx, &plans, repo_updated)
}

fn advance_worktrees(
    ctx: &Context,
    plans: &[RepositoryUpdatePlan],
    repo_updated: &RepositoryUpdateSignal,
) -> Result<TaskResult> {
    let mut changed = Vec::new();
    for (plan, commit) in plans
        .iter()
        .filter_map(|plan| Some((plan, plan.commit.as_deref()?)))
    {
        let repository = &plan.repository;
        let target = &repository.target;
        if checkout_head(ctx, repository)?.as_deref() != Some(plan.original_head.as_str())
            || worktree_has_local_changes(ctx, &target.root)?
        {
            let reason = checkout_changed(repository);
            return Ok(if changed.is_empty() {
                TaskResult::unmet(reason)
            } else {
                TaskResult::Failed(partial_merge_reason(&reason, &changed))
            });
        }
        // A branch's mergeOptions can enable squash even with --ff-only.
        let result = ctx.executor().execute(git_command(
            ctx,
            &target.root,
            &[
                "merge",
                "--ff-only",
                "--no-squash",
                "--no-autostash",
                "--no-overwrite-ignore",
                commit,
            ],
        ));
        match result {
            Ok(r) => {
                ctx.log()
                    .debug(format!("git merge output: {}", r.stdout.trim()));
                let head = checkout_head(ctx, repository).inspect_err(|_| {
                    repo_updated.mark_recovery_required();
                })?;
                if head.as_deref() != Some(commit) {
                    repo_updated.mark_recovery_required();
                    let reason = target.reason(
                        "git merge did not leave the expected branch at the fetched commit; inspect the checkout before retrying",
                    );
                    return Ok(TaskResult::Failed(partial_merge_reason(&reason, &changed)));
                }
                repo_updated.mark_updated();
                changed.push(target.description());
                if index_is_unmerged(ctx, &target.root)? {
                    let reason = target.reason(
                        "unmerged paths remain after fast-forward; inspect the checkout before retrying",
                    );
                    return Ok(TaskResult::Failed(partial_merge_reason(&reason, &changed)));
                }
                ctx.log().action(
                    "update",
                    target.description(),
                    false,
                    &format!("update {}", target.description()),
                );
            }
            Err(e) => {
                if e.is_cancelled() {
                    return Err(e.into());
                }
                // An unsuccessful merge can write files before updating HEAD
                // or the index. Stop even if Git reports no tracked changes:
                // files introduced by the merge may be untracked or ignored.
                repo_updated.mark_recovery_required();
                let merge_failure = target.reason("git merge --ff-only failed");
                ctx.log().warn(format!("{merge_failure}: {e:#}"));
                let state = checkout_head(ctx, repository).and_then(|head| {
                    Ok(head.as_deref() != Some(plan.original_head.as_str())
                        || worktree_has_local_changes(ctx, &target.root)?)
                });
                let detail = match state {
                    Ok(true) => "the checkout changed during the failed merge",
                    Ok(false) => "the failed merge may have left untracked files",
                    Err(error) if ctx.is_cancelled() => return Err(error),
                    Err(error) => {
                        ctx.log()
                            .warn(format!("could not inspect checkout: {error:#}"));
                        "the checkout state could not be verified"
                    }
                };
                let reason = partial_merge_reason(
                    &format!(
                        "{merge_failure}: {detail}. Further tasks stopped. In {}, inspect 'git status --short --ignored', 'git diff', and 'git diff --cached'. Preserve any edits you need, then repair the checkout before retrying",
                        target.root.display(),
                    ),
                    &changed,
                );
                return Ok(TaskResult::Failed(reason));
            }
        }
    }

    let changed = changed.len();
    Ok(if changed == 0 {
        TaskResult::Ok
    } else {
        TaskStats::from_counts(u32::try_from(changed).unwrap_or(u32::MAX), 0, 0, 0).finish()
    })
}

fn index_is_unmerged(ctx: &Context, root: &std::path::Path) -> Result<bool> {
    let result = ctx
        .executor()
        .execute(git_command(ctx, root, &["ls-files", "--unmerged"]))?;
    Ok(!result.stdout.is_empty())
}

fn checkout_changed(repository: &CheckedRepository) -> String {
    repository
        .target
        .reason("checkout branch, commit, or tracked files changed during update; retry")
}

fn partial_merge_reason(reason: &str, changed: &[&str]) -> String {
    if changed.is_empty() {
        reason.to_owned()
    } else {
        format!(
            "{reason}. Already advanced {}. Review the remaining checkout and retry update",
            changed.join(", ")
        )
    }
}

fn checkout_head(ctx: &Context, repository: &CheckedRepository) -> Result<Option<String>> {
    // rev-parse applies --symbolic-full-name only to arguments that follow it.
    // Detached HEAD has an object ID but no matching branch ref.
    let head = ctx.executor().execute(git_command(
        ctx,
        &repository.target.root,
        &["rev-parse", "HEAD", "--symbolic-full-name", "HEAD"],
    ))?;
    let mut lines = head.stdout.lines();
    Ok(match (lines.next(), lines.next()) {
        (Some(oid), Some(reference)) if reference.trim() == repository.head_ref => {
            Some(oid.trim().to_owned())
        }
        _ => None,
    })
}

/// Fetch every repository and capture the commit reported by each fetch.
/// Ordinary failures are reported in repository order.
///
/// Fetches are network-bound and mutually independent, so with more than one
/// repository they run concurrently: the overlay fetch no longer waits on the
/// main repository's round trip. This task gates the post-update process
/// restart, so the saving is on the run's critical path.
fn fetch_all(
    ctx: &Context,
    repositories: &[(&CheckedRepository, String, String)],
) -> std::result::Result<Result<Vec<String>, String>, ExecError> {
    use rayon::prelude::*;

    let fetch_one = |(repository, remote, reference): &(&CheckedRepository, String, String)| {
        ctx.log()
            .debug(format!("pulling from {}", repository.target.root.display()));
        let failure = |detail: &dyn std::fmt::Display| {
            let reason = repository.target.reason("git fetch failed");
            ctx.log().warn(format!("{reason}: {detail:#}"));
            Err(reason)
        };
        match fetch_with_retry(ctx, repository, remote, reference) {
            Ok(output) => Ok(fetched_commit(&output).map_or_else(
                || failure(&"git fetch did not report exactly one upstream object"),
                |commit| Ok(commit.to_owned()),
            )),
            Err(error) if error.is_cancelled() => Err(error),
            Err(error) => Ok(failure(&error)),
        }
    };

    if !ctx.parallel() || repositories.len() < 2 {
        let mut commits = Vec::with_capacity(repositories.len());
        for repository in repositories {
            match fetch_one(repository)? {
                Ok(commit) => commits.push(commit),
                Err(reason) => return Ok(Err(reason)),
            }
        }
        return Ok(Ok(commits));
    }

    // Rayon reuses threads across work items, so re-assert the diagnostic
    // thread name inside the closure to keep the log timeline attributable.
    let task_name = log_thread_name();
    // Collect before searching so the reported failure is the first in
    // declaration order rather than the first to complete: a fetch failure
    // then names the same repository whether or not the run is parallel.
    let outcomes = repositories
        .par_iter()
        .map(|repository| {
            set_log_thread_name(&task_name);
            fetch_one(repository)
        })
        .collect::<Vec<_>>();
    // A later cancellation must not be hidden by an earlier ordinary failure.
    let outcomes = outcomes
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(outcomes.into_iter().collect())
}

// Git can report the same object for FETCH_HEAD and its tracking refs. Pruned
// refs have a deletion flag and do not participate in the merge.
fn fetched_commit(output: &str) -> Option<&str> {
    let mut commit = None;
    for line in output.lines() {
        // A fast-forward uses a literal space as the status field.
        let (status, fields) = line.split_at_checked(1)?;
        let mut fields = fields.strip_prefix(' ')?.split_ascii_whitespace();
        match status {
            "-" => continue,
            " " | "+" | "t" | "*" | "=" => {}
            _ => return None,
        }
        let oid = fields.nth(1)?;
        if fields.next().is_none()
            || !matches!(oid.len(), 40 | 64)
            || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
            || commit.is_some_and(|previous| previous != oid)
        {
            return None;
        }
        commit = Some(oid);
    }
    commit
}

fn fetch_with_retry(
    ctx: &Context,
    repository: &CheckedRepository,
    remote: &str,
    reference: &str,
) -> std::result::Result<String, ExecError> {
    let mut attempt = 1_u32;
    loop {
        match ctx.executor().execute(
            git_command(
                ctx,
                &repository.target.root,
                &[
                    "fetch",
                    "--porcelain",
                    "--verbose",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "--",
                    remote,
                    reference,
                ],
            )
            .timeout(Duration::from_mins(5)),
        ) {
            Ok(result) => return Ok(result.stdout),
            Err(error) if error.is_cancelled() => return Err(error),
            Err(error) if attempt < MAX_FETCH_ATTEMPTS && is_transient_fetch_error(&error) => {
                ctx.log().warn(format!(
                    "transient git fetch failure for {} (attempt {attempt}/{MAX_FETCH_ATTEMPTS}), retrying: {error:#}",
                    repository.target.description()
                ));
                std::thread::sleep(fetch_retry_delay(attempt));
                attempt = attempt.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_transient_fetch_error(error: &ExecError) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    TRANSIENT_FETCH_ERROR_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

#[cfg(not(test))]
fn fetch_retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(u64::from(attempt))
}

#[cfg(test)]
const fn fetch_retry_delay(_attempt: u32) -> Duration {
    Duration::ZERO
}

/// Check whether the repository needs a merge by comparing HEAD with the
/// upstream ref, and reject diverged branches.
fn plan_repository_update(
    ctx: &Context,
    repository: &CheckedRepository,
    original_head: String,
    upstream_sha: &str,
) -> Result<RepositoryPlanReadiness> {
    let root = &repository.target.root;
    if checkout_head(ctx, repository)?.as_deref() != Some(original_head.as_str()) {
        return Ok(RepositoryPlanReadiness::Skipped(checkout_changed(
            repository,
        )));
    }
    let upstream = ctx.executor().execute(git_command(
        ctx,
        root,
        &[
            "rev-parse",
            "--verify",
            &format!("{upstream_sha}^{{commit}}"),
        ],
    ))?;
    let upstream = upstream.stdout.trim();
    let commit = match compare_commits(ctx, root, &original_head, upstream)? {
        CommitRelationship::AlreadyCurrent => {
            ctx.log().debug(format!(
                "{} already up to date",
                repository.target.description()
            ));
            None
        }
        CommitRelationship::WouldUpdate => Some(upstream.to_owned()),
        CommitRelationship::Ahead => {
            return Ok(RepositoryPlanReadiness::Skipped(
                repository
                    .target
                    .reason("local branch is ahead of upstream"),
            ));
        }
        CommitRelationship::Diverged => {
            return Ok(RepositoryPlanReadiness::Skipped(
                repository
                    .target
                    .reason("local branch diverged from upstream"),
            ));
        }
    };
    Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
        repository: repository.clone(),
        original_head,
        commit,
    }))
}
