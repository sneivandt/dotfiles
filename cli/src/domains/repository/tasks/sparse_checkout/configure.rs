//! Task: configure sparse checkout.
use anyhow::{Context as _, Result};
use std::path::Path;
use std::sync::Arc;

use crate::domains::repository::config::manifest::Manifest;
use crate::engine::{
    Context, Domain, Operation, OperationState, Task, TaskPhase, TaskResult, process_operation,
    task_metadata,
};
use crate::runtime::ConfigHandle;
use crate::runtime::fs::{FileSystemOps, SystemFileSystemOps};

/// Default sparse checkout pattern that includes all files at root level.
const DEFAULT_SPARSE_PATTERN: &str = "/*";

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
        .run_unchecked_in(root, "git", &["config", "--get", "core.sparseCheckout"])
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
pub(super) fn enable_sparse_checkout_config(ctx: &Context, root: &Path) -> Result<()> {
    ctx.log()
        .debug("enabling sparse checkout (non-cone mode via git config)");
    let scope: &[&str] = if worktree_config_enabled(ctx, root) {
        &["--worktree"]
    } else {
        &[]
    };
    set_git_config(ctx, root, scope, "core.sparseCheckout", "true")?;
    set_git_config(ctx, root, scope, "core.sparseCheckoutCone", "false")?;
    Ok(())
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
    ctx.executor().run_in(root, "git", &args)?;
    Ok(())
}

/// Return whether the `extensions.worktreeConfig` extension is enabled, in
/// which case `core.*` overrides live in the per-worktree config scope.
fn worktree_config_enabled(ctx: &Context, root: &Path) -> bool {
    ctx.executor()
        .run_unchecked_in(
            root,
            "git",
            &["config", "--get", "extensions.worktreeConfig"],
        )
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
    if let Err(e) = ctx.executor().run_in(root, "git", &checkout_args) {
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
) -> Result<()> {
    ctx.log()
        .debug("wrote sparse-checkout file, running read-tree");
    if let Err(err) = ctx
        .executor()
        .run_in(root, "git", &["read-tree", "-mu", "HEAD"])
    {
        ctx.log()
            .warn("git read-tree failed; restoring previous sparse-checkout configuration");
        restore_sparse_checkout_file(sparse_file, previous_patterns)?;
        ctx.executor()
            .run_in(root, "git", &["read-tree", "-mu", "HEAD"])
            .context("restoring worktree after failed sparse-checkout update")?;
        return Err(err.context("applying sparse-checkout patterns"));
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
}

impl ConfigureSparseCheckout {
    /// Create using the real filesystem and a handle to the manifest config.
    #[must_use]
    pub fn new(config: ConfigHandle<Manifest>) -> Self {
        Self {
            fs_ops: Arc::new(SystemFileSystemOps),
            config,
        }
    }

    /// Create with a custom [`FileSystemOps`] implementation (for testing).
    #[cfg(test)]
    pub fn with_fs_ops(fs_ops: Arc<dyn FileSystemOps>, config: ConfigHandle<Manifest>) -> Self {
        Self { fs_ops, config }
    }
}

impl Task for ConfigureSparseCheckout {
    task_metadata! {
        name: "Configure sparse checkout",
        phase: TaskPhase::Sync,
        domain: Domain::Repository,
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run if git is available and we're in a git repo
        self.fs_ops.exists(&ctx.root().join(".git"))
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_operation(
            ctx,
            &SparseCheckoutOperation {
                fs_ops: Arc::clone(&self.fs_ops),
                config: self.config.clone(),
            },
        )
    }
}

#[derive(Debug, Clone)]
struct SparseCheckoutOperation {
    fs_ops: Arc<dyn FileSystemOps>,
    config: ConfigHandle<Manifest>,
}

impl Operation for SparseCheckoutOperation {
    type Plan = Vec<String>;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let excluded_files: Vec<String> = self.config.read().excluded_files.clone();

        if excluded_files.is_empty() {
            ctx.log().info("no files to exclude from sparse checkout");
            return Ok(OperationState::Complete);
        }

        let patterns_str = build_patterns(&excluded_files);
        let sparse_file = ctx.root().join(".git/info/sparse-checkout");

        if is_up_to_date(&sparse_file, &patterns_str)
            && sparse_checkout_config_enabled(ctx, ctx.root())
        {
            ctx.log().debug(&format!(
                "already configured ({} files excluded)",
                excluded_files.len()
            ));
            return Ok(OperationState::Complete);
        }

        Ok(OperationState::needs_run(
            "configure sparse checkout",
            excluded_files,
        ))
    }

    fn preview(&self, ctx: &Context, excluded_files: &Self::Plan) -> Result<TaskResult> {
        ctx.log().dry_run("configure git sparse checkout");
        for file in excluded_files {
            ctx.log().dry_run(&format!("  exclude: {file}"));
        }
        Ok(TaskResult::DryRun)
    }

    fn apply(&self, ctx: &Context, excluded_files: &Self::Plan) -> Result<TaskResult> {
        let patterns_str = build_patterns(excluded_files);
        let sparse_file = ctx.root().join(".git/info/sparse-checkout");

        // Clean up broken git config symlinks that prevent git from running.
        remove_broken_git_symlinks(ctx, &*self.fs_ops);

        if worktree_has_local_changes(ctx)? {
            return Ok(TaskResult::Skipped("local changes present".to_string()));
        }

        let previous_patterns = read_existing_patterns(&sparse_file)?;

        let root = ctx.root();

        enable_sparse_checkout_config(ctx, root)?;

        ctx.debug_fmt(|| {
            format!(
                "sparse checkout patterns: 1 inclusion, {} exclusions",
                excluded_files.len()
            )
        });

        write_sparse_patterns(&sparse_file, &patterns_str)?;
        reset_excluded_to_head(ctx, root, excluded_files);
        apply_read_tree_with_restore(ctx, root, &sparse_file, previous_patterns.as_deref())?;

        ctx.log().info(&format!(
            "excluded {} files from checkout",
            excluded_files.len()
        ));

        Ok(TaskResult::Ok)
    }
}

pub(super) fn worktree_has_local_changes(ctx: &Context) -> Result<bool> {
    let status = ctx.executor().run_in(
        ctx.root(),
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
    )?;

    Ok(!status.stdout.trim().is_empty())
}
