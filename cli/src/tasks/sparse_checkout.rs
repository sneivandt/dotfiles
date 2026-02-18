use anyhow::Result;
use std::path::Path;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Default sparse checkout pattern that includes all files at root level.
const DEFAULT_SPARSE_PATTERN: &str = "/*";

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
    match std::fs::read_link(path) {
        Ok(target) if target.starts_with(dir) => !target.exists(),
        _ => false,
    }
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
pub struct SparseCheckout;

impl Task for SparseCheckout {
    fn name(&self) -> &'static str {
        "Configure sparse checkout"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run if git is available and we're in a git repo
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.dry_run {
            if ctx.config.manifest.excluded_files.is_empty() {
                ctx.log.info("no files to exclude from sparse checkout");
                return Ok(TaskResult::Ok);
            }

            // Check if sparse-checkout patterns are already up to date
            let mut patterns = vec![DEFAULT_SPARSE_PATTERN.to_string()];
            for file in &ctx.config.manifest.excluded_files {
                patterns.push(format!("!/{file}"));
            }
            let patterns_str = patterns.join("\n");
            let sparse_file = ctx.root().join(".git/info/sparse-checkout");
            if sparse_file.exists() {
                let current = std::fs::read_to_string(&sparse_file).unwrap_or_default();
                if current.trim() == patterns_str.trim() {
                    ctx.log.info(&format!(
                        "already configured ({} files excluded)",
                        ctx.config.manifest.excluded_files.len()
                    ));
                    return Ok(TaskResult::Ok);
                }
            }

            ctx.log.dry_run("configure git sparse checkout");
            for file in &ctx.config.manifest.excluded_files {
                ctx.log.dry_run(&format!("  exclude: {file}"));
            }
            return Ok(TaskResult::DryRun);
        }

        // Clean up broken git config symlinks that prevent git from running.
        remove_broken_git_symlinks(ctx);

        let root = ctx.root();

        // Enable sparse checkout
        ctx.log.debug("initializing sparse checkout (cone mode)");
        exec::run_in(root, "git", &["sparse-checkout", "init", "--cone"])?;

        // Build the sparse checkout patterns
        if ctx.config.manifest.excluded_files.is_empty() {
            ctx.log
                .debug("manifest has no excluded files, nothing to configure");
            ctx.log.info("no files to exclude from sparse checkout");
            return Ok(TaskResult::Ok);
        }

        // Disable cone mode to use full pattern matching
        ctx.log
            .debug("switching to non-cone mode for pattern matching");
        exec::run_in(root, "git", &["sparse-checkout", "init", "--no-cone"])?;

        // Write patterns: include everything except excluded files
        let mut patterns = vec![DEFAULT_SPARSE_PATTERN.to_string()];
        for file in &ctx.config.manifest.excluded_files {
            patterns.push(format!("!/{file}"));
        }

        let patterns_str = patterns.join("\n");
        ctx.log.debug(&format!(
            "sparse checkout patterns: {} inclusions, {} exclusions",
            1,
            ctx.config.manifest.excluded_files.len()
        ));

        // Check if sparse-checkout patterns are already up to date
        let info_dir = root.join(".git/info");
        let sparse_file = info_dir.join("sparse-checkout");
        if sparse_file.exists() {
            let current = std::fs::read_to_string(&sparse_file).unwrap_or_default();
            if current.trim() == patterns_str.trim() {
                ctx.log.debug("sparse checkout patterns already up to date");
                ctx.log.info(&format!(
                    "already configured ({} files excluded)",
                    ctx.config.manifest.excluded_files.len()
                ));
                return Ok(TaskResult::Ok);
            }
            ctx.log.debug("sparse checkout patterns differ, updating");
        } else {
            ctx.log
                .debug("sparse checkout file does not exist, creating");
        }

        // Write directly to sparse-checkout file
        if !info_dir.exists() {
            std::fs::create_dir_all(&info_dir)?;
        }
        std::fs::write(&sparse_file, &patterns_str)?;

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
