pub mod chmod;
pub mod copilot_skills;
pub mod fonts;
pub mod git_config;
pub mod hooks;
pub mod packages;
pub mod registry;
pub mod shell;
pub mod sparse_checkout;
pub mod symlinks;
pub mod systemd;
pub mod update;
pub mod vscode;

use anyhow::Result;
use std::path::Path;

use crate::config::Config;
use crate::logging::{Logger, TaskStatus};
use crate::platform::Platform;

/// Shared context for task execution.
pub struct Context<'a> {
    pub config: &'a Config,
    pub platform: &'a Platform,
    pub log: &'a Logger,
    pub dry_run: bool,
    #[allow(dead_code)]
    pub verbose: bool,
    pub home: std::path::PathBuf,
}

impl<'a> Context<'a> {
    pub fn new(
        config: &'a Config,
        platform: &'a Platform,
        log: &'a Logger,
        dry_run: bool,
        verbose: bool,
    ) -> Self {
        let home = if cfg!(target_os = "windows") {
            std::env::var("USERPROFILE")
                .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_default())
        } else {
            std::env::var("HOME").unwrap_or_default()
        };

        Self {
            config,
            platform,
            log,
            dry_run,
            verbose,
            home: std::path::PathBuf::from(home),
        }
    }

    /// Root directory of the dotfiles repository.
    pub fn root(&self) -> &Path {
        &self.config.root
    }

    /// Symlinks source directory.
    pub fn symlinks_dir(&self) -> std::path::PathBuf {
        self.config.root.join("symlinks")
    }

    /// Hooks source directory.
    pub fn hooks_dir(&self) -> std::path::PathBuf {
        self.config.root.join("hooks")
    }
}

/// Result of a single task execution.
pub enum TaskResult {
    /// Task completed successfully.
    Ok,
    /// Task was skipped (not applicable to this platform/profile).
    Skipped(String),
    /// Task ran in dry-run mode.
    DryRun,
}

/// A named, executable task.
pub trait Task {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Execute the task.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}

/// Execute a task, recording the result in the logger.
pub fn execute(task: &dyn Task, ctx: &Context) {
    if !task.should_run(ctx) {
        ctx.log
            .record_task(task.name(), TaskStatus::Skipped, Some("not applicable"));
        return;
    }

    ctx.log.stage(task.name());

    match task.run(ctx) {
        Ok(TaskResult::Ok) => {
            ctx.log.record_task(task.name(), TaskStatus::Ok, None);
        }
        Ok(TaskResult::Skipped(reason)) => {
            ctx.log.info(&format!("skipped: {reason}"));
            ctx.log
                .record_task(task.name(), TaskStatus::Skipped, Some(&reason));
        }
        Ok(TaskResult::DryRun) => {
            ctx.log.record_task(task.name(), TaskStatus::DryRun, None);
        }
        Err(e) => {
            ctx.log.error(&format!("{}: {e:#}", task.name()));
            ctx.log
                .record_task(task.name(), TaskStatus::Failed, Some(&format!("{e:#}")));
        }
    }
}
