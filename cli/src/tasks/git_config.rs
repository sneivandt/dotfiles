use anyhow::Result;

use super::{Context, Task, TaskResult};
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

        let mut already_ok = 0u32;
        let mut would_change = 0u32;

        for &(key, desired) in settings {
            let current = exec::run_unchecked("git", &["config", "--global", "--get", key])
                .map(|r| r.stdout.trim().to_string())
                .unwrap_or_default();

            if current == desired {
                ctx.log
                    .debug(&format!("ok: {key} = {desired} (already set)"));
                already_ok += 1;
            } else {
                if ctx.dry_run {
                    ctx.log.dry_run(&format!("would set {key} = {desired}"));
                } else {
                    exec::run("git", &["config", "--global", key, desired])?;
                }
                would_change += 1;
            }
        }

        if ctx.dry_run {
            ctx.log.info(&format!(
                "{would_change} would change, {already_ok} already ok"
            ));
            return Ok(TaskResult::DryRun);
        }

        ctx.log
            .info(&format!("{would_change} changed, {already_ok} already ok"));
        Ok(TaskResult::Ok)
    }
}
