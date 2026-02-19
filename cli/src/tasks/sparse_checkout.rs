use anyhow::{Context as _, Result};
use std::path::Path;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Default sparse checkout pattern that includes all files at root level.
const DEFAULT_SPARSE_PATTERN: &str = "/*";

/// Build the sparse checkout pattern string from excluded files.
fn build_patterns(excluded_files: &[String]) -> String {
    let mut patterns = vec![DEFAULT_SPARSE_PATTERN.to_string()];
    for file in excluded_files {
        patterns.push(format!("!/{file}"));
    }
    patterns.join("\n")
}

/// Check if the sparse-checkout file is already up to date with the given patterns.
fn is_up_to_date(sparse_file: &Path, patterns_str: &str) -> bool {
    if !sparse_file.exists() {
        return false;
    }
    std::fs::read_to_string(sparse_file)
        .map(|current| current.trim() == patterns_str.trim())
        .unwrap_or(false)
}

/// Remove broken symlinks in `~/.config/git/` that point into the dotfiles
/// repo's `symlinks/` directory.  These become dangling when sparse-checkout
/// excludes `symlinks/`, which then prevents git from running at all because
/// it cannot read its own XDG config / exclude files.
fn remove_broken_git_symlinks(ctx: &Context) {
    let git_config_dir = ctx.home.join(".config").join("git");
    if !git_config_dir.exists() {
        return;
    }
    let symlinks_dir = ctx.symlinks_dir();
    let Ok(entries) = std::fs::read_dir(&git_config_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_broken_symlink_into(&path, &symlinks_dir) {
            continue;
        }
        ctx.log.debug(&format!(
            "removing broken git config symlink: {}",
            path.display()
        ));
        if let Err(e) = remove_path(&path) {
            ctx.log.debug(&format!("failed to remove symlink: {e}"));
        }
    }
}

/// Returns true when `path` is a symlink whose target lives under `dir` and
/// the target does not exist on disk.
fn is_broken_symlink_into(path: &Path, dir: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_symlink() => {}
        _ => return false,
    }
    std::fs::read_link(path).is_ok_and(|target| {
        // Resolve relative symlink targets relative to the symlink's directory
        let resolved_target = if target.is_absolute() {
            target
        } else {
            path.parent()
                .map_or_else(|| target.clone(), |parent| parent.join(&target))
        };
        resolved_target.starts_with(dir) && !resolved_target.exists()
    })
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Configure git sparse checkout based on the profile manifest.
#[derive(Debug)]
pub struct ConfigureSparseCheckout;

impl Task for ConfigureSparseCheckout {
    fn name(&self) -> &'static str {
        "Configure sparse checkout"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run if git is available and we're in a git repo
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.config.manifest.excluded_files.is_empty() {
            ctx.log.info("no files to exclude from sparse checkout");
            return Ok(TaskResult::Ok);
        }

        let patterns_str = build_patterns(&ctx.config.manifest.excluded_files);
        let sparse_file = ctx.root().join(".git/info/sparse-checkout");

        // Check if patterns are already up to date (shared by dry-run and real paths)
        if is_up_to_date(&sparse_file, &patterns_str) {
            ctx.log.info(&format!(
                "already configured ({} files excluded)",
                ctx.config.manifest.excluded_files.len()
            ));
            return Ok(TaskResult::Ok);
        }

        if ctx.dry_run {
            ctx.log.dry_run("configure git sparse checkout");
            for file in &ctx.config.manifest.excluded_files {
                ctx.log.dry_run(&format!("  exclude: {file}"));
            }
            return Ok(TaskResult::DryRun);
        }

        // Clean up broken git config symlinks that prevent git from running.
        remove_broken_git_symlinks(ctx);

        let root = ctx.root();

        // Enable sparse checkout with non-cone mode for full pattern matching.
        // Non-cone mode supports negation patterns (e.g., !/<file>) which are
        // needed to selectively exclude files.
        ctx.log
            .debug("initializing sparse checkout (non-cone mode)");
        exec::run_in(root, "git", &["sparse-checkout", "init", "--no-cone"])?;

        ctx.log.debug(&format!(
            "sparse checkout patterns: 1 inclusion, {} exclusions",
            ctx.config.manifest.excluded_files.len()
        ));

        // Write directly to sparse-checkout file
        let info_dir = root.join(".git/info");
        if !info_dir.exists() {
            std::fs::create_dir_all(&info_dir).context("creating .git/info directory")?;
        }
        std::fs::write(&sparse_file, &patterns_str).context("writing sparse-checkout file")?;

        // Reset excluded files to HEAD so read-tree doesn't fail with
        // "not uptodate. Cannot merge." when the working tree is dirty.
        let mut checkout_args = vec!["checkout", "HEAD", "--"];
        let excluded: Vec<&str> = ctx
            .config
            .manifest
            .excluded_files
            .iter()
            .filter(|f| root.join(f).exists())
            .map(String::as_str)
            .collect();
        if !excluded.is_empty() {
            checkout_args.extend(&excluded);
            ctx.log.debug(&format!(
                "resetting {} excluded files to HEAD before read-tree",
                excluded.len()
            ));
            // Best-effort: if checkout fails (e.g. file not in HEAD), proceed anyway
            if let Err(e) = exec::run_in(root, "git", &checkout_args) {
                ctx.log.debug(&format!("git checkout reset failed: {e}"));
            }
        }

        ctx.log
            .debug("wrote sparse-checkout file, running read-tree");
        exec::run_in(root, "git", &["read-tree", "-mu", "HEAD"])?;

        ctx.log.info(&format!(
            "excluded {} files from checkout",
            ctx.config.manifest.excluded_files.len()
        ));

        Ok(TaskResult::Ok)
    }
}
