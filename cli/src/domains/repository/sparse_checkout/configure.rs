//! Task: configure sparse checkout.
use anyhow::{Context as _, Result, bail};
use std::path::Path;
use std::sync::Arc;

use crate::domains::repository::config::manifest::Manifest;
use crate::engine::{
    Context, Operation, OperationState, Task, TaskResult, TaskStats, process_operation,
    task_metadata,
};
use crate::infra::ConfigHandle;
use crate::infra::exec::CommandSpec;
use crate::infra::fs::{FileSystemOps, SystemFileSystemOps};
use crate::infra::logging::OutputExt as _;

/// Default sparse checkout pattern that includes all files at root level.
const DEFAULT_SPARSE_PATTERN: &str = "/*";

fn git_command(root: &Path, args: &[&str]) -> CommandSpec {
    CommandSpec::new("git").args(args).current_dir(root)
}

/// Build the sparse checkout pattern string from excluded files.
pub(super) fn build_patterns(excluded_files: &[String]) -> String {
    let mut patterns = vec![DEFAULT_SPARSE_PATTERN.to_string()];
    for file in excluded_files {
        patterns.push(format!("!/symlinks/{file}"));
    }
    patterns.join("\n")
}

/// Check if the sparse-checkout file is already up to date with the given patterns.
pub(super) fn is_up_to_date(sparse_file: &Path, patterns_str: &str) -> bool {
    if !sparse_file.exists() {
        return false;
    }
    std::fs::read_to_string(sparse_file).is_ok_and(|current| current.trim() == patterns_str.trim())
}

/// Return whether `core.sparseCheckout` is currently enabled in the repo.
///
/// A matching `.git/info/sparse-checkout` file is not sufficient to consider
/// sparse checkout applied: `git sparse-checkout disable` (or a manual
/// `git config core.sparseCheckout false`) flips this flag to `false` while
/// leaving the file intact, and git then ignores the patterns entirely.
/// Checking the flag lets [`ConfigureSparseCheckout::run`] re-enable sparse
/// checkout instead of short-circuiting on the still-matching file.
fn sparse_checkout_config_enabled(ctx: &Context, root: &Path) -> bool {
    ctx.executor()
        .execute(git_command(root, &["config", "--get", "core.sparseCheckout"]).unchecked())
        .is_ok_and(|result| result.success && result.stdout.trim() == "true")
}

/// Read the existing sparse-checkout file contents, if any.
///
/// Returns `Ok(None)` when the file does not exist.
fn read_existing_patterns(sparse_file: &Path) -> Result<Option<String>> {
    if !sparse_file.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(sparse_file)
        .map(Some)
        .with_context(|| format!("reading {}", sparse_file.display()))
}

/// Enable non-cone sparse checkout by setting git config directly.
///
/// Using `git sparse-checkout init --no-cone` is avoided here because it
/// overwrites the sparse-checkout file with default `/*\n!/*/\n` patterns
/// and immediately applies them via an internal `git read-tree`, deleting
/// every repository subdirectory from the working tree.  If the process
/// that invoked this binary inherited a cwd from inside the repository
/// (e.g. a CI script running from `.github/workflows/scripts/`), that
/// directory is deleted and its inode becomes unreachable.  Any child
/// process spawned later (such as `gh copilot plugin list`) inherits the
/// stale cwd and fails with `ENOENT: uv_cwd` when Node.js calls
/// `process.cwd()` during startup.
///
/// Setting the two config keys directly enables sparse checkout in
/// non-cone mode without modifying the working tree; the subsequent
/// `git read-tree -mu HEAD` then applies only our intentional patterns.
///
/// The keys are written to the per-worktree config scope when the
/// `extensions.worktreeConfig` extension is active.  `git sparse-checkout
/// disable` enables that extension and stores `core.sparseCheckout=false` in
/// the worktree config, which overrides the repository scope; writing plain
/// `git config` there would be silently shadowed and sparse checkout would
/// never re-enable.
#[cfg(test)]
pub(super) fn enable_sparse_checkout_config(ctx: &Context, root: &Path) -> Result<()> {
    ctx.log()
        .debug("enabling sparse checkout (non-cone mode via git config)");
    let scope = sparse_checkout_config_scope(ctx, root);
    enable_sparse_checkout_config_in_scope(ctx, root, scope)?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum SparseCheckoutConfigScope {
    Repository,
    Worktree,
}

impl SparseCheckoutConfigScope {
    const fn args(self) -> &'static [&'static str] {
        match self {
            Self::Repository => &["--local"],
            Self::Worktree => &["--worktree"],
        }
    }
}

#[derive(Debug, Clone)]
struct SparseCheckoutConfigSnapshot {
    scope: SparseCheckoutConfigScope,
    enabled: Option<String>,
    cone: Option<String>,
}

fn sparse_checkout_config_scope(ctx: &Context, root: &Path) -> SparseCheckoutConfigScope {
    if worktree_config_enabled(ctx, root) {
        SparseCheckoutConfigScope::Worktree
    } else {
        SparseCheckoutConfigScope::Repository
    }
}

fn enable_sparse_checkout_config_in_scope(
    ctx: &Context,
    root: &Path,
    scope: SparseCheckoutConfigScope,
) -> Result<()> {
    set_git_config(ctx, root, scope.args(), "core.sparseCheckout", "true")?;
    set_git_config(ctx, root, scope.args(), "core.sparseCheckoutCone", "false")
}

fn read_git_config_value(
    ctx: &Context,
    root: &Path,
    scope: SparseCheckoutConfigScope,
    key: &str,
) -> Result<Option<String>> {
    let mut args = vec!["config"];
    args.extend_from_slice(scope.args());
    args.extend(["--get", key]);
    let result = ctx
        .executor()
        .execute(git_command(root, &args).unchecked())
        .with_context(|| format!("reading git config {key}"))?;
    if result.success {
        return Ok(Some(result.stdout.trim().to_string()));
    }
    if result.code == Some(1) {
        return Ok(None);
    }
    bail!(
        "reading git config {key} failed with exit {:?}: {}",
        result.code,
        result.stderr.trim()
    )
}

fn capture_sparse_checkout_config(
    ctx: &Context,
    root: &Path,
) -> Result<SparseCheckoutConfigSnapshot> {
    let scope = sparse_checkout_config_scope(ctx, root);
    Ok(SparseCheckoutConfigSnapshot {
        scope,
        enabled: read_git_config_value(ctx, root, scope, "core.sparseCheckout")?,
        cone: read_git_config_value(ctx, root, scope, "core.sparseCheckoutCone")?,
    })
}

fn restore_git_config_value(
    ctx: &Context,
    root: &Path,
    scope: SparseCheckoutConfigScope,
    key: &str,
    value: Option<&str>,
) -> Result<()> {
    if let Some(value) = value {
        return set_git_config(ctx, root, scope.args(), key, value);
    }

    let mut args = vec!["config"];
    args.extend_from_slice(scope.args());
    args.extend(["--unset", key]);
    ctx.executor().execute(git_command(root, &args))?;
    Ok(())
}

fn restore_sparse_checkout_config(
    ctx: &Context,
    root: &Path,
    snapshot: &SparseCheckoutConfigSnapshot,
) -> Result<()> {
    let enabled = restore_git_config_value(
        ctx,
        root,
        snapshot.scope,
        "core.sparseCheckout",
        snapshot.enabled.as_deref(),
    );
    let cone = restore_git_config_value(
        ctx,
        root,
        snapshot.scope,
        "core.sparseCheckoutCone",
        snapshot.cone.as_deref(),
    );
    enabled.and(cone)
}

/// Write a single git config key/value in the repository at `root`, using the
/// extra `scope` flags (e.g. `--worktree`) when supplied.
fn set_git_config(
    ctx: &Context,
    root: &Path,
    scope: &[&str],
    key: &str,
    value: &str,
) -> Result<()> {
    let mut args = vec!["config"];
    args.extend_from_slice(scope);
    args.push(key);
    args.push(value);
    ctx.executor().execute(git_command(root, &args))?;
    Ok(())
}

/// Return whether the `extensions.worktreeConfig` extension is enabled, in
/// which case `core.*` overrides live in the per-worktree config scope.
fn worktree_config_enabled(ctx: &Context, root: &Path) -> bool {
    ctx.executor()
        .execute(git_command(root, &["config", "--get", "extensions.worktreeConfig"]).unchecked())
        .is_ok_and(|result| result.success && result.stdout.trim() == "true")
}

/// Write the patterns string to `.git/info/sparse-checkout`, creating the
/// parent directory if needed.
fn write_sparse_patterns(sparse_file: &Path, patterns_str: &str) -> Result<()> {
    if let Some(info_dir) = sparse_file.parent()
        && !info_dir.exists()
    {
        std::fs::create_dir_all(info_dir).context("creating .git/info directory")?;
    }
    std::fs::write(sparse_file, patterns_str).context("writing sparse-checkout file")?;
    Ok(())
}

/// Reset excluded files to HEAD so a subsequent `read-tree` doesn't fail with
/// "not uptodate. Cannot merge." when the working tree is dirty.
///
/// Best-effort: failures are logged at debug level and otherwise ignored
/// (e.g. when an excluded file isn't tracked in HEAD).
pub(super) fn reset_excluded_to_head(ctx: &Context, root: &Path, excluded_files: &[String]) {
    let excluded: Vec<String> = excluded_files
        .iter()
        .filter_map(|f| {
            let repo_path = format!("symlinks/{f}");
            root.join(&repo_path).exists().then_some(repo_path)
        })
        .collect();
    if excluded.is_empty() {
        return;
    }
    let mut checkout_args = vec!["checkout", "HEAD", "--"];
    checkout_args.extend(excluded.iter().map(String::as_str));
    ctx.debug_fmt(|| {
        format!(
            "resetting {} excluded files to HEAD before read-tree",
            excluded.len()
        )
    });
    if let Err(e) = ctx.executor().execute(git_command(root, &checkout_args)) {
        ctx.debug_fmt(|| format!("git checkout reset failed: {e}"));
    }
}

/// Run `git read-tree -mu HEAD` to apply the new sparse-checkout patterns.
///
/// On failure, restore the previous sparse-checkout file contents and run
/// `read-tree` again to put the working tree back to a consistent state,
/// then return the original error.
fn apply_read_tree_with_restore(
    ctx: &Context,
    root: &Path,
    sparse_file: &Path,
    previous_patterns: Option<&str>,
    previous_config: &SparseCheckoutConfigSnapshot,
) -> Result<()> {
    ctx.log()
        .debug("wrote sparse-checkout file, running read-tree");
    if let Err(err) = ctx
        .executor()
        .execute(git_command(root, &["read-tree", "-mu", "HEAD"]))
    {
        ctx.log()
            .warn("git read-tree failed; restoring previous sparse-checkout configuration");
        let patterns_restore = restore_sparse_checkout_file(sparse_file, previous_patterns);
        let config_restore = restore_sparse_checkout_config(ctx, root, previous_config);
        let worktree_restore = ctx
            .executor()
            .execute(git_command(root, &["read-tree", "-mu", "HEAD"]))
            .context("restoring worktree after failed sparse-checkout update")
            .map(drop);
        let rollback = patterns_restore.and(config_restore).and(worktree_restore);
        let apply_error = anyhow::Error::from(err).context("applying sparse-checkout patterns");
        return match rollback {
            Ok(()) => Err(apply_error),
            Err(rollback_error) => {
                Err(apply_error.context(format!("rollback also failed: {rollback_error:#}")))
            }
        };
    }
    Ok(())
}

pub(super) fn restore_sparse_checkout_file(
    sparse_file: &Path,
    previous_patterns: Option<&str>,
) -> Result<()> {
    if let Some(previous) = previous_patterns {
        std::fs::write(sparse_file, previous).with_context(|| {
            format!(
                "restoring sparse-checkout file at {}",
                sparse_file.display()
            )
        })
    } else {
        if let Err(err) = std::fs::remove_file(sparse_file)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(err).with_context(|| {
                format!("removing sparse-checkout file at {}", sparse_file.display())
            });
        }
        Ok(())
    }
}

/// Remove broken symlinks in `~/.config/git/` that point into the dotfiles
/// repo's `symlinks/` directory.  These become dangling when sparse-checkout
/// excludes `symlinks/`, which then prevents git from running at all because
/// it cannot read its own XDG config / exclude files.
pub(super) fn remove_broken_git_symlinks(ctx: &Context, fs: &dyn FileSystemOps) {
    let paths = ctx.paths();
    let git_config_dir = paths.home().join(".config").join("git");
    if !fs.exists(&git_config_dir) {
        return;
    }
    let symlinks_dir = paths.symlinks_dir();
    let Ok(entries) = fs.read_dir(&git_config_dir) else {
        return;
    };
    for path in entries {
        if !is_broken_symlink_into(fs, &path, symlinks_dir) {
            continue;
        }
        ctx.debug_fmt(|| format!("removing broken git config symlink: {}", path.display()));
        if let Err(e) = fs.remove(&path) {
            ctx.debug_fmt(|| format!("failed to remove symlink: {e}"));
        }
    }
}

/// Returns true when `path` is a symlink whose target lives under `dir` and
/// the target does not exist on disk.
pub(super) fn is_broken_symlink_into(fs: &dyn FileSystemOps, path: &Path, dir: &Path) -> bool {
    fs.read_link(path).is_ok_and(|target| {
        // Resolve relative symlink targets relative to the symlink's directory
        let resolved_target = if target.is_absolute() {
            target
        } else {
            path.parent()
                .map_or_else(|| target.clone(), |parent| parent.join(&target))
        };
        resolved_target.starts_with(dir) && !fs.exists(&resolved_target)
    })
}

/// Configure git sparse checkout based on the profile manifest.
#[derive(Debug)]
pub struct ConfigureSparseCheckout {
    fs_ops: Arc<dyn FileSystemOps>,
    config: ConfigHandle<Manifest>,
    fail_if_skipped: bool,
}

impl ConfigureSparseCheckout {
    /// Create using the real filesystem and a handle to the manifest config.
    #[must_use]
    pub fn new(config: ConfigHandle<Manifest>) -> Self {
        Self {
            fs_ops: Arc::new(SystemFileSystemOps),
            config,
            fail_if_skipped: false,
        }
    }

    /// Require reconciliation to complete after a repository-triggered restart.
    #[must_use]
    pub const fn fail_if_skipped(mut self, fail_if_skipped: bool) -> Self {
        self.fail_if_skipped = fail_if_skipped;
        self
    }

    /// Create with a custom [`FileSystemOps`] implementation (for testing).
    #[cfg(test)]
    pub fn with_fs_ops(fs_ops: Arc<dyn FileSystemOps>, config: ConfigHandle<Manifest>) -> Self {
        Self {
            fs_ops,
            config,
            fail_if_skipped: false,
        }
    }
}

impl Task for ConfigureSparseCheckout {
    task_metadata! {
        name: "Sparse checkout",
        selector: "sparse-checkout",
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run if git is available and we're in a git repo
        self.fs_ops.exists(&ctx.root().join(".git"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let result = process_operation(
            ctx,
            &SparseCheckoutOperation {
                fs_ops: Arc::clone(&self.fs_ops),
                config: self.config.clone(),
            },
        )?;
        if self.fail_if_skipped
            && let TaskResult::Skipped { reason, .. } = &result
        {
            return Ok(TaskResult::Failed(format!(
                "post-update sparse checkout reconciliation skipped: {reason}"
            )));
        }
        Ok(result)
    }
}

#[derive(Debug, Clone)]
struct SparseCheckoutOperation {
    fs_ops: Arc<dyn FileSystemOps>,
    config: ConfigHandle<Manifest>,
}

#[derive(Debug, Clone)]
enum SparseCheckoutPlan {
    Configure(Vec<String>),
    Disable,
    Skip(String),
}

impl Operation for SparseCheckoutOperation {
    type Plan = SparseCheckoutPlan;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let excluded_files: Vec<String> = self.config.read().excluded_files.clone();

        if excluded_files.is_empty() {
            if sparse_checkout_config_enabled(ctx, ctx.root()) {
                if worktree_has_local_changes(ctx)? {
                    return Ok(OperationState::needs_run(
                        "local changes present",
                        SparseCheckoutPlan::Skip("local changes present".to_string()),
                    ));
                }
                return Ok(OperationState::needs_run(
                    "disable sparse checkout",
                    SparseCheckoutPlan::Disable,
                ));
            }
            ctx.log().info("no files to exclude from sparse checkout");
            return Ok(OperationState::Complete);
        }

        let patterns_str = build_patterns(&excluded_files);
        let sparse_file = ctx.root().join(".git/info/sparse-checkout");

        if is_up_to_date(&sparse_file, &patterns_str)
            && sparse_checkout_config_enabled(ctx, ctx.root())
        {
            ctx.log().debug(format!(
                "already configured ({} files excluded)",
                excluded_files.len()
            ));
            return Ok(OperationState::Complete);
        }

        if worktree_has_local_changes(ctx)? {
            return Ok(OperationState::needs_run(
                "local changes present",
                SparseCheckoutPlan::Skip("local changes present".to_string()),
            ));
        }

        Ok(OperationState::needs_run(
            "configure sparse checkout",
            SparseCheckoutPlan::Configure(excluded_files),
        ))
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        match plan {
            SparseCheckoutPlan::Configure(excluded_files) => {
                ctx.log().dry_run("configure git sparse checkout");
                for file in excluded_files {
                    ctx.log().dry_run(format!("  exclude: {file}"));
                }
                Ok(TaskStats::changed().finish())
            }
            SparseCheckoutPlan::Disable => {
                ctx.log().dry_run("disable git sparse checkout");
                Ok(TaskStats::changed().finish())
            }
            SparseCheckoutPlan::Skip(reason) => Ok(TaskResult::unmet(reason.clone())),
        }
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        let excluded_files = match plan {
            SparseCheckoutPlan::Configure(excluded_files) => excluded_files,
            SparseCheckoutPlan::Disable => {
                remove_broken_git_symlinks(ctx, &*self.fs_ops);
                ctx.executor()
                    .execute(git_command(ctx.root(), &["sparse-checkout", "disable"]))?;
                ctx.log().info("disabled sparse checkout");
                return Ok(TaskStats::changed().finish());
            }
            SparseCheckoutPlan::Skip(reason) => {
                return Ok(TaskResult::unmet(reason.clone()));
            }
        };

        let patterns_str = build_patterns(excluded_files);
        let sparse_file = ctx.root().join(".git/info/sparse-checkout");

        // Clean up broken git config symlinks that prevent git from running.
        remove_broken_git_symlinks(ctx, &*self.fs_ops);

        let previous_patterns = read_existing_patterns(&sparse_file)?;
        let previous_config = capture_sparse_checkout_config(ctx, ctx.root())?;

        let root = ctx.root();

        if let Err(error) = enable_sparse_checkout_config_in_scope(ctx, root, previous_config.scope)
        {
            return match restore_sparse_checkout_config(ctx, root, &previous_config) {
                Ok(()) => Err(error),
                Err(rollback_error) => {
                    Err(error.context(format!("restoring git config failed: {rollback_error:#}")))
                }
            };
        }

        ctx.debug_fmt(|| {
            format!(
                "sparse checkout patterns: 1 inclusion, {} exclusions",
                excluded_files.len()
            )
        });

        if let Err(error) = write_sparse_patterns(&sparse_file, &patterns_str) {
            let patterns_restore =
                restore_sparse_checkout_file(&sparse_file, previous_patterns.as_deref());
            let config_restore = restore_sparse_checkout_config(ctx, root, &previous_config);
            return match patterns_restore.and(config_restore) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error.context(format!(
                    "restoring sparse checkout failed: {rollback_error:#}"
                ))),
            };
        }
        reset_excluded_to_head(ctx, root, excluded_files);
        apply_read_tree_with_restore(
            ctx,
            root,
            &sparse_file,
            previous_patterns.as_deref(),
            &previous_config,
        )?;

        ctx.log().info(format!(
            "excluded {} files from checkout",
            excluded_files.len()
        ));

        Ok(TaskResult::Ok)
    }
}

pub(super) fn worktree_has_local_changes(ctx: &Context) -> Result<bool> {
    let status = ctx.executor().execute(git_command(
        ctx.root(),
        &["status", "--porcelain", "--untracked-files=no"],
    ))?;

    Ok(!status.stdout.trim().is_empty())
}
