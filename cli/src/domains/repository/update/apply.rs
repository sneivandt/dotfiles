//! Execution: fetch, divergence check, and fast-forward merge.
use std::time::Duration;

use anyhow::Result;

use crate::engine::{Context, TaskResult, UpdateSignal};

use super::models::{CheckedRepository, RepositoryPlanReadiness, RepositoryUpdatePlan};

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
    git_env: &[(&str, &str)],
    repo_updated: &UpdateSignal,
) -> Result<TaskResult> {
    for repository in repositories {
        ctx.log().debug(&format!(
            "pulling from {}",
            repository.target.root.display()
        ));

        // Fetch first so divergence can be evaluated without invoking `git pull`,
        // which fails noisily when the local branch has diverged from upstream.
        if let Err(e) = fetch_with_retry(ctx, repository, git_env) {
            let reason = repository.target.reason("git fetch failed");
            ctx.log().warn(&format!("{reason}: {e:#}"));
            return Ok(TaskResult::Failed(reason));
        }
    }

    let mut plans = Vec::with_capacity(repositories.len());
    for repository in repositories {
        match plan_repository_update(ctx, repository, git_env)? {
            RepositoryPlanReadiness::Ready(plan) => plans.push(plan),
            RepositoryPlanReadiness::Skipped(reason) => return Ok(TaskResult::Skipped(reason)),
        }
    }

    let mut updated = false;
    for plan in plans.iter().filter(|plan| plan.needs_update) {
        let result = ctx.executor().run_in_with_env(
            &plan.target.root,
            "git",
            &["merge", "--ff-only", "@{u}"],
            git_env,
        );
        match result {
            Ok(r) => {
                ctx.log()
                    .debug(&format!("git merge output: {}", r.stdout.trim()));
                ctx.log()
                    .info(&format!("{} updated", plan.target.description()));
                updated = true;
            }
            Err(e) => {
                let reason = plan.target.reason("git merge --ff-only failed");
                ctx.log().warn(&format!("{reason}: {e:#}"));
                return Ok(TaskResult::Failed(reason));
            }
        }
    }

    if updated {
        repo_updated.mark_updated();
    }
    Ok(TaskResult::Ok)
}

fn fetch_with_retry(
    ctx: &Context,
    repository: &CheckedRepository,
    git_env: &[(&str, &str)],
) -> Result<()> {
    let mut attempt = 1_u32;
    loop {
        match ctx.executor().run_in_with_env(
            &repository.target.root,
            "git",
            &["fetch", "--quiet"],
            git_env,
        ) {
            Ok(_) => return Ok(()),
            Err(error) if attempt < MAX_FETCH_ATTEMPTS && is_transient_fetch_error(&error) => {
                ctx.log().warn(&format!(
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

fn is_transient_fetch_error(error: &anyhow::Error) -> bool {
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
    git_env: &[(&str, &str)],
) -> Result<RepositoryPlanReadiness> {
    let pre_sha = ctx
        .executor()
        .run_in_with_env(
            &repository.target.root,
            "git",
            &["rev-parse", "HEAD"],
            git_env,
        )?
        .stdout
        .trim()
        .to_string();

    let upstream_sha = match ctx.executor().run_in_with_env(
        &repository.target.root,
        "git",
        &["rev-parse", "@{u}"],
        git_env,
    ) {
        Ok(r) => r.stdout.trim().to_string(),
        Err(e) => {
            let reason = repository.target.reason("no upstream tracking branch");
            ctx.log().warn(&format!("{reason}: {e:#}"));
            return Ok(RepositoryPlanReadiness::Skipped(reason));
        }
    };

    if pre_sha == upstream_sha {
        ctx.log().debug(&format!(
            "{} already up to date",
            repository.target.description()
        ));
        return Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
            target: repository.target.clone(),
            needs_update: false,
        }));
    }

    // Detect a diverged or local-only branch by counting commits on HEAD
    // that are not on upstream. A non-zero count means `git pull --ff-only`
    // would fail; skip rather than report a hard failure.
    let ahead_output = ctx
        .executor()
        .run_in_with_env(
            &repository.target.root,
            "git",
            &["rev-list", "--count", "@{u}..HEAD"],
            git_env,
        )?
        .stdout
        .trim()
        .to_string();
    let ahead = match ahead_output.parse::<u64>() {
        Ok(ahead) => ahead,
        Err(error) => {
            let reason = repository
                .target
                .reason("could not determine whether the local branch diverged");
            ctx.log().warn(&format!(
                "{reason}: invalid rev-list count {ahead_output:?}: {error}"
            ));
            return Ok(RepositoryPlanReadiness::Skipped(reason));
        }
    };

    if ahead > 0 {
        let reason = repository
            .target
            .reason("local branch diverged from upstream");
        ctx.log().info(&format!("{reason}, skipping pull"));
        return Ok(RepositoryPlanReadiness::Skipped(reason));
    }

    Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
        target: repository.target.clone(),
        needs_update: true,
    }))
}
