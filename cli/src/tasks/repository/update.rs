//! Task: update the dotfiles repository.
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::tasks::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, UpdateSignal,
    process_operation, task_metadata,
};

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
        deps: [crate::tasks::repository::sparse_checkout::ConfigureSparseCheckout],
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
    repositories: Mutex<Option<Vec<CheckedRepository>>>,
}

impl UpdateRepositoryOperation {
    const fn new(repo_updated: UpdateSignal) -> Self {
        Self {
            repo_updated,
            repositories: Mutex::new(None),
        }
    }

    fn repositories(
        &self,
        ctx: &Context,
        git_env: &[(&str, &str)],
    ) -> Result<RepositorySetReadiness> {
        {
            let cached = self
                .repositories
                .lock()
                .map_err(|_| anyhow::anyhow!("repository update state cache is poisoned"))?;
            if let Some(repositories) = cached.as_ref() {
                return Ok(RepositorySetReadiness::Ready(repositories.clone()));
            }
        }

        match checked_repositories(ctx, git_env)? {
            RepositorySetReadiness::Ready(repositories) => {
                let mut cached = self
                    .repositories
                    .lock()
                    .map_err(|_| anyhow::anyhow!("repository update state cache is poisoned"))?;
                *cached = Some(repositories.clone());
                drop(cached);
                Ok(RepositorySetReadiness::Ready(repositories))
            }
            skipped @ RepositorySetReadiness::Skipped(_) => Ok(skipped),
        }
    }
}

impl Operation for UpdateRepositoryOperation {
    fn current_state(&self, ctx: &Context) -> Result<OperationState> {
        let home_str = ctx.home.to_string_lossy().into_owned();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];
        match self.repositories(ctx, git_env)? {
            RepositorySetReadiness::Ready(repositories) if repositories.is_empty() => {
                Ok(OperationState::Complete)
            }
            RepositorySetReadiness::Ready(_) => {
                Ok(OperationState::needs_run("update repositories"))
            }
            RepositorySetReadiness::Skipped(reason) => Ok(OperationState::blocked(reason)),
        }
    }

    fn preview(&self, ctx: &Context, _state: &OperationState) -> Result<TaskResult> {
        let home_str = ctx.home.to_string_lossy().into_owned();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];
        match self.repositories(ctx, git_env)? {
            RepositorySetReadiness::Ready(repositories) => {
                dry_run_repositories(ctx, &repositories, git_env)
            }
            RepositorySetReadiness::Skipped(reason) => Ok(TaskResult::Skipped(reason)),
        }
    }

    fn apply(&self, ctx: &Context, _state: &OperationState) -> Result<TaskResult> {
        let home_str = ctx.home.to_string_lossy().into_owned();
        let git_env: &[(&str, &str)] = &[("HOME", &home_str), ("GIT_CONFIG_NOSYSTEM", "1")];
        match self.repositories(ctx, git_env)? {
            RepositorySetReadiness::Ready(repositories) => {
                apply_repository_updates(ctx, repositories, git_env, &self.repo_updated)
            }
            RepositorySetReadiness::Skipped(reason) => Ok(TaskResult::Skipped(reason)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateTargetKind {
    Main,
    Overlay,
}

#[derive(Debug, Clone)]
struct UpdateTarget {
    kind: UpdateTargetKind,
    root: PathBuf,
}

impl UpdateTarget {
    const fn new(kind: UpdateTargetKind, root: PathBuf) -> Self {
        Self { kind, root }
    }

    const fn description(&self) -> &'static str {
        match self.kind {
            UpdateTargetKind::Main => "repository",
            UpdateTargetKind::Overlay => "overlay repository",
        }
    }

    fn reason(&self, reason: &str) -> String {
        match self.kind {
            UpdateTargetKind::Main => reason.to_string(),
            UpdateTargetKind::Overlay => format!("{reason} in {}", self.description()),
        }
    }

    fn dry_run_action(&self) -> String {
        match self.kind {
            UpdateTargetKind::Main => "git pull".to_string(),
            UpdateTargetKind::Overlay => format!("git pull ({})", self.description()),
        }
    }
}

#[derive(Debug, Clone)]
struct CheckedRepository {
    target: UpdateTarget,
    head_ref: String,
}

#[derive(Debug)]
enum RepositoryReadiness {
    Ready(CheckedRepository),
    Skipped(String),
}

#[derive(Debug)]
enum RepositorySetReadiness {
    Ready(Vec<CheckedRepository>),
    Skipped(String),
}

#[derive(Debug)]
struct RepositoryUpdatePlan {
    target: UpdateTarget,
    needs_update: bool,
}

#[derive(Debug)]
enum RepositoryPlanReadiness {
    Ready(RepositoryUpdatePlan),
    Skipped(String),
}

fn update_targets(ctx: &Context) -> Vec<UpdateTarget> {
    let config = ctx.config_read();
    let mut targets = vec![UpdateTarget::new(
        UpdateTargetKind::Main,
        config.root.clone(),
    )];

    if let Some(overlay) = &config.overlay
        && overlay.join(".git").exists()
    {
        targets.push(UpdateTarget::new(
            UpdateTargetKind::Overlay,
            overlay.clone(),
        ));
    }

    targets
}

fn checked_repositories(ctx: &Context, git_env: &[(&str, &str)]) -> Result<RepositorySetReadiness> {
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

fn apply_repository_updates(
    ctx: &Context,
    repositories: Vec<CheckedRepository>,
    git_env: &[(&str, &str)],
    repo_updated: &UpdateSignal,
) -> Result<TaskResult> {
    for repository in &repositories {
        ctx.log.debug(&format!(
            "pulling from {}",
            repository.target.root.display()
        ));

        // Fetch first so divergence can be evaluated without invoking `git pull`,
        // which fails noisily when the local branch has diverged from upstream.
        if let Err(e) = ctx.executor.run_in_with_env(
            &repository.target.root,
            "git",
            &["fetch", "--quiet"],
            git_env,
        ) {
            let reason = repository.target.reason("git fetch failed");
            ctx.log.warn(&format!("{reason}: {e:#}"));
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
        let result = ctx.executor.run_in_with_env(
            &plan.target.root,
            "git",
            &["merge", "--ff-only", "@{u}"],
            git_env,
        );
        match result {
            Ok(r) => {
                ctx.log
                    .debug(&format!("git merge output: {}", r.stdout.trim()));
                ctx.log
                    .info(&format!("{} updated", plan.target.description()));
                updated = true;
            }
            Err(e) => {
                let reason = plan.target.reason("git merge --ff-only failed");
                ctx.log.warn(&format!("{reason}: {e:#}"));
                return Ok(TaskResult::Failed(reason));
            }
        }
    }

    if updated {
        repo_updated.mark_updated();
    }
    Ok(TaskResult::Ok)
}

fn check_repository_ready(
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

fn dry_run_repositories(
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

fn plan_repository_update(
    ctx: &Context,
    repository: CheckedRepository,
    git_env: &[(&str, &str)],
) -> Result<RepositoryPlanReadiness> {
    let pre_sha = ctx
        .executor
        .run_in_with_env(
            &repository.target.root,
            "git",
            &["rev-parse", "HEAD"],
            git_env,
        )?
        .stdout
        .trim()
        .to_string();

    let upstream_sha = match ctx.executor.run_in_with_env(
        &repository.target.root,
        "git",
        &["rev-parse", "@{u}"],
        git_env,
    ) {
        Ok(r) => r.stdout.trim().to_string(),
        Err(e) => {
            let reason = repository.target.reason("no upstream tracking branch");
            ctx.log.warn(&format!("{reason}: {e:#}"));
            return Ok(RepositoryPlanReadiness::Skipped(reason));
        }
    };

    if pre_sha == upstream_sha {
        ctx.log.debug(&format!(
            "{} already up to date",
            repository.target.description()
        ));
        return Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
            target: repository.target,
            needs_update: false,
        }));
    }

    // Detect a diverged or local-only branch by counting commits on HEAD
    // that are not on upstream. A non-zero count means `git pull --ff-only`
    // would fail; skip rather than report a hard failure.
    let ahead_output = ctx
        .executor
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
            ctx.log.warn(&format!(
                "{reason}: invalid rev-list count {ahead_output:?}: {error}"
            ));
            return Ok(RepositoryPlanReadiness::Skipped(reason));
        }
    };

    if ahead > 0 {
        let reason = repository
            .target
            .reason("local branch diverged from upstream");
        ctx.log.info(&format!("{reason}, skipping pull"));
        return Ok(RepositoryPlanReadiness::Skipped(reason));
    }

    Ok(RepositoryPlanReadiness::Ready(RepositoryUpdatePlan {
        target: repository.target,
        needs_update: true,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DryRunUpdateStatus {
    AlreadyCurrent,
    WouldUpdate,
    Unknown,
}

fn dry_run_update_status(
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

fn upstream_remote_sha(
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

fn worktree_has_local_changes(
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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::exec::{ExecResult, Executor, MockExecutor};
    use crate::platform::{Os, Platform};
    use crate::tasks::UpdateSignal;
    use crate::tasks::test_helpers::{empty_config, make_context, make_linux_context};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn ok_result(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        }
    }

    fn expect_git_success(
        mock: &mut MockExecutor,
        seq: &mut mockall::Sequence,
        expected_dir: PathBuf,
        expected_args: &'static [&'static str],
        stdout: &'static str,
    ) {
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(seq)
            .returning(move |dir, program, args, _| {
                assert_eq!(dir, expected_dir.as_path());
                assert_eq!(program, "git");
                assert_eq!(args, expected_args);
                Ok(ok_result(stdout))
            });
    }

    #[test]
    fn should_run_false_when_git_dir_missing() {
        let config = empty_config(PathBuf::from("/nonexistent/repo"));
        let ctx = make_linux_context(config);
        let task = UpdateRepository::new(UpdateSignal::new());
        assert!(!task.should_run(&ctx));
    }

    #[test]
    fn should_run_true_when_git_dir_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        let task = UpdateRepository::new(UpdateSignal::new());
        assert!(task.should_run(&ctx));
    }

    /// In a git worktree the repo root contains a `.git` *file* (not a
    /// directory) that stores the path to the per-worktree git data.
    /// `should_run` must return `true` in this layout.
    #[test]
    fn should_run_true_when_git_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".git"), "gitdir: ../.git/worktrees/my-wt\n").unwrap();
        let config = empty_config(dir.path().to_path_buf());
        let ctx = make_linux_context(config);
        let task = UpdateRepository::new(UpdateSignal::new());
        assert!(task.should_run(&ctx));
    }

    // -----------------------------------------------------------------------
    // run()
    // -----------------------------------------------------------------------

    /// Build a context that uses a [`MockExecutor`] so we can control git responses.
    fn make_update_context(config: crate::config::Config, executor: MockExecutor) -> Context {
        make_context(config, Platform::new(Os::Linux, false), Arc::new(executor))
    }

    #[test]
    fn run_returns_skipped_when_detached_head() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): fails → detached HEAD
        let mut mock = MockExecutor::new();
        mock.expect_run_in_with_env()
            .once()
            .returning(|_, _, _, _| Err(anyhow::anyhow!("simulated failure")));
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("detached HEAD")));
        assert!(!repo_updated.was_updated());
    }

    #[test]
    fn run_skips_when_staged_changes_detected() {
        let config = empty_config(PathBuf::from("/tmp"));
        // First call (symbolic-ref): succeeds → on a branch
        // Second call (status --porcelain): returns non-empty stdout → local changes
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _, _| Ok(ok_result("refs/heads/main")));
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _, _| Ok(ok_result("M  dirty_file.txt")));
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated);

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes")));
    }

    #[derive(Debug)]
    struct UntrackedAwareExecutor;

    impl Executor for UntrackedAwareExecutor {
        fn run(&self, _: &str, _: &[&str]) -> Result<ExecResult> {
            anyhow::bail!("unexpected run() call")
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            _: &str,
            args: &[&str],
            _: &[(&str, &str)],
        ) -> Result<ExecResult> {
            let stdout = if args.contains(&"--untracked-files=no") {
                String::new()
            } else {
                "?? new-file.txt\n".to_string()
            };

            Ok(ExecResult {
                stdout,
                stderr: String::new(),
                success: true,
                code: Some(0),
            })
        }

        fn run_unchecked(&self, _: &str, _: &[&str]) -> Result<ExecResult> {
            anyhow::bail!("unexpected run_unchecked() call")
        }

        fn which(&self, _: &str) -> bool {
            false
        }

        fn which_path(&self, program: &str) -> Result<PathBuf> {
            anyhow::bail!("{program} not found on PATH")
        }
    }

    #[test]
    fn worktree_has_local_changes_ignores_untracked_files() {
        let config = empty_config(PathBuf::from("/repo"));
        let ctx = make_context(
            config,
            Platform::new(Os::Linux, false),
            Arc::new(UntrackedAwareExecutor),
        );

        assert!(!worktree_has_local_changes(&ctx, Path::new("/repo"), &[]).unwrap());
    }

    #[test]
    fn run_returns_ok_and_does_not_mark_updated_when_already_up_to_date() {
        let config = empty_config(PathBuf::from("/tmp"));
        // 1. symbolic-ref → on a branch
        // 2. status → clean worktree
        // 3. fetch → ok
        // 4. rev-parse HEAD → SHA
        // 5. rev-parse @{u} → same SHA → already up to date
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in ["refs/heads/main", "", "", "abc123", "abc123"] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(!repo_updated.was_updated());
    }

    #[test]
    fn run_returns_ok_and_marks_updated_when_pull_fetches_new_commits() {
        let config = empty_config(PathBuf::from("/tmp"));
        // 1. symbolic-ref → on a branch
        // 2. status → clean worktree
        // 3. fetch → ok
        // 4. rev-parse HEAD → pre-merge SHA
        // 5. rev-parse @{u} → newer SHA
        // 6. rev-list --count @{u}..HEAD → 0 (not ahead)
        // 7. merge --ff-only → succeeds
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in [
            "refs/heads/main",
            "",
            "",
            "abc1234",
            "def5678",
            "0",
            "Updating abc1234..def5678\nFast-forward",
        ] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(repo_updated.was_updated());
    }

    #[test]
    fn run_returns_skipped_when_local_branch_diverged() {
        let config = empty_config(PathBuf::from("/tmp"));
        // 1. symbolic-ref → on a branch
        // 2. status → clean worktree
        // 3. fetch → ok
        // 4. rev-parse HEAD → SHA
        // 5. rev-parse @{u} → different SHA
        // 6. rev-list --count @{u}..HEAD → 2 (local commits ahead → diverged)
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in ["refs/heads/main", "", "", "abc1234", "def5678", "2"] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Skipped(ref s) if s.contains("diverged")));
        assert!(!repo_updated.was_updated());
    }

    #[test]
    fn run_returns_skipped_when_rev_list_count_is_malformed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in [
            "refs/heads/main",
            "",
            "",
            "abc1234",
            "def5678",
            "not-a-count",
        ] {
            let output = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&output)));
        }
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();

        assert!(
            matches!(result, TaskResult::Skipped(ref reason) if reason.contains("could not determine"))
        );
        assert!(!repo_updated.was_updated());
    }

    #[test]
    fn run_returns_failed_when_fetch_fails() {
        let config = empty_config(PathBuf::from("/tmp"));
        // 1. symbolic-ref → on a branch
        // 2. status → clean worktree
        // 3. fetch → fails
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in ["refs/heads/main", ""] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _, _| Err(anyhow::anyhow!("simulated fetch failure")));
        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated);

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Failed(ref s) if s.contains("git fetch failed")));
    }

    #[test]
    fn run_skips_when_overlay_has_local_changes() {
        let main_root = PathBuf::from("/tmp/main");
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir(overlay.path().join(".git")).unwrap();
        let overlay_root = overlay.path().to_path_buf();
        let mut config = empty_config(main_root.clone());
        config.overlay = Some(overlay_root.clone());

        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root,
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root,
            &["status", "--porcelain", "--untracked-files=no"],
            "M  private.toml",
        );

        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Skipped(ref s) if s.contains("local changes") && s.contains("overlay"))
        );
        assert!(!repo_updated.was_updated());
    }

    #[test]
    fn run_updates_overlay_repository_when_behind_upstream() {
        let main_root = PathBuf::from("/tmp/main");
        let overlay = tempfile::tempdir().unwrap();
        std::fs::create_dir(overlay.path().join(".git")).unwrap();
        let overlay_root = overlay.path().to_path_buf();
        let mut config = empty_config(main_root.clone());
        config.overlay = Some(overlay_root.clone());

        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root.clone(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["symbolic-ref", "--quiet", "HEAD"],
            "refs/heads/main",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["status", "--porcelain", "--untracked-files=no"],
            "",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root.clone(),
            &["fetch", "--quiet"],
            "",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["fetch", "--quiet"],
            "",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root.clone(),
            &["rev-parse", "HEAD"],
            "abc123",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            main_root,
            &["rev-parse", "@{u}"],
            "abc123",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["rev-parse", "HEAD"],
            "def456",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["rev-parse", "@{u}"],
            "fed654",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root.clone(),
            &["rev-list", "--count", "@{u}..HEAD"],
            "0",
        );
        expect_git_success(
            &mut mock,
            &mut seq,
            overlay_root,
            &["merge", "--ff-only", "@{u}"],
            "Updating def456..fed654\nFast-forward",
        );

        let ctx = make_update_context(config, mock);
        let repo_updated = UpdateSignal::new();
        let task = UpdateRepository::new(repo_updated.clone());

        let result = task.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::Ok));
        assert!(repo_updated.was_updated());
    }

    // -----------------------------------------------------------------------
    // run() — dry-run comparison paths
    // -----------------------------------------------------------------------

    #[test]
    fn run_dry_run_returns_ok_when_already_up_to_date() {
        let config = empty_config(PathBuf::from("/tmp"));
        // symbolic-ref: success → on a branch
        // status --porcelain: empty → clean worktree
        // rev-parse HEAD: abc123
        // branch.main.remote: origin
        // branch.main.merge: refs/heads/main
        // ls-remote origin refs/heads/main: abc123
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in [
            "refs/heads/main",
            "",
            "abc123",
            "origin",
            "refs/heads/main",
            "abc123\trefs/heads/main",
        ] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        let mut ctx = make_update_context(config, mock);
        ctx = ctx.with_dry_run(true);
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok (already up to date in dry-run), got {result:?}"
        );
    }

    #[test]
    fn run_dry_run_returns_dry_run_when_behind_upstream() {
        let config = empty_config(PathBuf::from("/tmp"));
        // symbolic-ref: success
        // status --porcelain: empty
        // rev-parse HEAD: abc123
        // branch.main.remote: origin
        // branch.main.merge: refs/heads/main
        // ls-remote origin refs/heads/main: def456 (different SHA → would pull)
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in [
            "refs/heads/main",
            "",
            "abc123",
            "origin",
            "refs/heads/main",
            "def456\trefs/heads/main",
        ] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        let mut ctx = make_update_context(config, mock);
        ctx = ctx.with_dry_run(true);
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::DryRun),
            "expected DryRun (behind upstream), got {result:?}"
        );
    }

    #[test]
    fn run_dry_run_returns_ok_when_cached_upstream_matches_head() {
        let config = empty_config(PathBuf::from("/tmp"));
        // symbolic-ref: success
        // status --porcelain: empty
        // rev-parse HEAD: abc123
        // branch.main.remote lookup fails
        // rev-parse @{u}: abc123 (cached tracking ref matches HEAD)
        let mut seq = mockall::Sequence::new();
        let mut mock = MockExecutor::new();
        for stdout in ["refs/heads/main", "", "abc123"] {
            let s = stdout.to_string();
            mock.expect_run_in_with_env()
                .once()
                .in_sequence(&mut seq)
                .returning(move |_, _, _, _| Ok(ok_result(&s)));
        }
        // branch.main.remote lookup fails
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _, _| Err(anyhow::anyhow!("no remote config")));
        // rev-parse @{u}: matches HEAD
        mock.expect_run_in_with_env()
            .once()
            .in_sequence(&mut seq)
            .returning(|_, _, _, _| Ok(ok_result("abc123")));
        let mut ctx = make_update_context(config, mock);
        ctx = ctx.with_dry_run(true);
        let task = UpdateRepository::new(UpdateSignal::new());

        let result = task.run(&ctx).unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok when cached upstream matches HEAD, got {result:?}"
        );
    }
}
