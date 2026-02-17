use anyhow::Result;

use super::{Context, Task, TaskResult, TaskStats};
use crate::exec;

/// Configure git settings (Windows-specific git config).
pub struct ConfigureGit;

impl Task for ConfigureGit {
    fn name(&self) -> &'static str {
        "Configure git"
    }

    fn should_run(&self, ctx: &Context) -> bool {
        ctx.platform.is_windows() && exec::which("git")
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let settings: &[(&str, &str)] = &[
            ("core.autocrlf", "false"),
            ("core.symlinks", "true"),
            ("credential.helper", "manager"),
        ];

        let mut stats = TaskStats::new();

        for &(key, desired) in settings {
            let current = exec::run_unchecked("git", &["config", "--global", "--get", key])
                .map(|r| r.stdout.trim().to_string())
                .unwrap_or_default();

            if current == desired {
                ctx.log
                    .debug(&format!("ok: {key} = {desired} (already set)"));
                stats.already_ok += 1;
            } else {
                if ctx.dry_run {
                    ctx.log.dry_run(&format!("would set {key} = {desired}"));
                } else {
                    exec::run("git", &["config", "--global", key, desired])?;
                }
                stats.changed += 1;
            }
        }

        Ok(stats.finish(ctx))
    }
}
