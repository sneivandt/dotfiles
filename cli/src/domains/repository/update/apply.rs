//! Execution: fetch, divergence check, and fast-forward merge.
use std::time::Duration;

use anyhow::Result;

use crate::engine::{Context, TaskResult, TaskStats};
use crate::infra::exec::ExecError;

use super::discovery::upstream;
use super::models::{CheckedRepository, RepositoryPlanReadiness, RepositoryUpdatePlan};
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
    if let Some(reason) = fetch_all(ctx, &tracked)? {
        return Ok(TaskResult::Failed(reason));
    }

    let mut plans = Vec::with_capacity(repositories.len());
    for repository in repositories {
        match plan_repository_update(ctx, repository)? {
            RepositoryPlanReadiness::Ready(plan) => plans.push(plan),
            RepositoryPlanReadiness::Skipped(reason) => return Ok(TaskResult::unmet(reason)),
        }
    }

    let mut changed = Vec::new();
    for (plan, commit) in plans
        .iter()
        .filter_map(|plan| Some((plan, plan.commit.as_deref()?)))
    {
        let result = ctx.executor().execute(git_command(
            ctx,
            &plan.target.root,
            &["merge", "--ff-only", "--no-overwrite-ignore", commit],
        ));
        match result {
            Ok(r) => {
                ctx.log()
                    .debug(format!("git merge output: {}", r.stdout.trim()));
                ctx.log()
                    .info(format!("update {}", plan.target.description()));
                repo_updated.mark_updated();
                changed.push(plan.target.description());
            }
            Err(e) => {
                if e.is_cancelled() {
                    return Err(e.into());
                }
                let merge_failure = plan.target.reason("git merge --ff-only failed");
                let reason = if changed.is_empty() {
                    merge_failure
                } else {
                    format!(
                        "{merge_failure}. Already advanced {}. Resolve the failed merge and retry update",
                        changed.join(", ")
                    )
                };
                ctx.log().warn(format!("{reason}: {e:#}"));
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

/// Fetch every repository, returning the failure reason for the first one that
/// could not be fetched.
///
/// Fetches are network-bound and mutually independent, so with more than one
/// repository they run concurrently: the overlay fetch no longer waits on the
/// main repository's round trip. This task gates the post-update process
/// restart, so the saving is on the run's critical path.
fn fetch_all(
    ctx: &Context,
    repositories: &[(&CheckedRepository, String, String)],
) -> std::result::Result<Option<String>, ExecError> {
    use rayon::prelude::*;

    let fetch_one =
        |(repository, remote, reference): &(&CheckedRepository, String, String)| -> std::result::Result<Option<String>, ExecError> {
            ctx.log()
                .debug(format!("pulling from {}", repository.target.root.display()));
            match fetch_with_retry(ctx, repository, remote, reference) {
                Ok(()) => Ok(None),
                Err(error) if error.is_cancelled() => Err(error),
                Err(error) => {
                    let reason = repository.target.reason("git fetch failed");
                    ctx.log().warn(format!("{reason}: {error:#}"));
                    Ok(Some(reason))
                }
            }
        };

    if !ctx.parallel() || repositories.len() < 2 {
        for repository in repositories {
            if let Some(reason) = fetch_one(repository)? {
                return Ok(Some(reason));
            }
        }
        return Ok(None);
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
    Ok(outcomes.into_iter().flatten().next())
}

fn fetch_with_retry(
    ctx: &Context,
    repository: &CheckedRepository,
    remote: &str,
    reference: &str,
) -> std::result::Result<(), ExecError> {
    let mut attempt = 1_u32;
    loop {
        match ctx.executor().execute(
            git_command(
                ctx,
                &repository.target.root,
                &["fetch", "--quiet", "--no-tags", "--", remote, reference],
            )
            .timeout(Duration::from_mins(5)),
        ) {
            Ok(_) => return Ok(()),
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
pub(super) fn plan_repository_update(
    ctx: &Context,
    repository: &CheckedRepository,
) -> Result<RepositoryPlanReadiness> {
    let pre_sha = ctx
        .executor()
        .execute(git_command(
            ctx,
            &repository.target.root,
            &["rev-parse", "HEAD"],
        ))?
        .stdout
        .trim()
        .to_string();

    let upstream_sha = ctx
        .executor()
        .execute(git_command(
            ctx,
            &repository.target.root,
            &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
        ))?
        .stdout
        .trim()
        .to_string();

    if pre_sha == upstream_sha {
        ctx.log().debug(format!(
            "{} already up to date",
            repository.target.description()
        ));
        return Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
            target: repository.target.clone(),
            commit: None,
        }));
    }

    // Detect a diverged or local-only branch by counting commits on HEAD
    // that are not on upstream. A non-zero count means `git pull --ff-only`
    // would fail; skip rather than report a hard failure.
    let ahead_output = ctx
        .executor()
        .execute(git_command(
            ctx,
            &repository.target.root,
            &["rev-list", "--count", &format!("{upstream_sha}..{pre_sha}")],
        ))?
        .stdout
        .trim()
        .to_string();
    let ahead = match ahead_output.parse::<u64>() {
        Ok(ahead) => ahead,
        Err(error) => {
            let reason = repository
                .target
                .reason("could not determine whether the local branch diverged");
            ctx.log().warn(format!(
                "{reason}: invalid rev-list count {ahead_output:?}: {error}"
            ));
            return Ok(RepositoryPlanReadiness::Skipped(reason));
        }
    };

    if ahead > 0 {
        let reason = repository
            .target
            .reason("local branch diverged from upstream");
        return Ok(RepositoryPlanReadiness::Skipped(reason));
    }

    Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
        target: repository.target.clone(),
        commit: Some(upstream_sha),
    }))
}
