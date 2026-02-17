use anyhow::Result;

use super::{Context, Task, TaskResult};
use crate::exec;

/// Configure git sparse checkout based on the profile manifest.
pub struct SparseCheckout;

impl Task for SparseCheckout {
    fn name(&self) -> &str {
        "Configure sparse checkout"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        // Only run if git is available and we're in a git repo
        ctx.root().join(".git").exists()
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.dry_run {
            ctx.log.dry_run("configure git sparse checkout");
            for file in &ctx.config.manifest.excluded_files {
                ctx.log.dry_run(&format!("  exclude: {file}"));
            }
            return Ok(TaskResult::DryRun);
        }

        let root = ctx.root();

        // Enable sparse checkout
        exec::run_in(root, "git", &["sparse-checkout", "init", "--cone"])?;

        // Build the sparse checkout patterns
        if ctx.config.manifest.excluded_files.is_empty() {
            ctx.log.info("no files to exclude from sparse checkout");
            return Ok(TaskResult::Ok);
        }

        // Disable cone mode to use full pattern matching
        exec::run_in(root, "git", &["sparse-checkout", "init", "--no-cone"])?;

        // Write patterns: include everything except excluded files
        let mut patterns = vec!["/*".to_string()];
        for file in &ctx.config.manifest.excluded_files {
            patterns.push(format!("!/{file}"));
        }

        let patterns_str = patterns.join("\n");

        // Write directly to sparse-checkout file
        let info_dir = root.join(".git/info");
        if !info_dir.exists() {
            std::fs::create_dir_all(&info_dir)?;
        }
        std::fs::write(info_dir.join("sparse-checkout"), &patterns_str)?;
        exec::run_in(root, "git", &["read-tree", "-mu", "HEAD"])?;

        ctx.log.info(&format!(
            "excluded {} files from checkout",
            ctx.config.manifest.excluded_files.len()
        ));

        Ok(TaskResult::Ok)
    }
}
