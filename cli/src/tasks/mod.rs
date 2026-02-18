pub mod chmod;
pub mod copilot_skills;
pub mod developer_mode;
pub mod git_config;
pub mod hooks;
pub mod packages;
pub mod registry;
pub mod shell;
pub mod sparse_checkout;
pub mod symlinks;
pub mod systemd_units;
pub mod update;
pub mod vscode_extensions;

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
    pub home: std::path::PathBuf,
}

impl<'a> Context<'a> {
    pub fn new(
        config: &'a Config,
        platform: &'a Platform,
        log: &'a Logger,
        dry_run: bool,
    ) -> Result<Self> {
        let home = if cfg!(target_os = "windows") {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map_err(|_| {
                    anyhow::anyhow!("neither USERPROFILE nor HOME environment variable is set")
                })?
        } else {
            std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("HOME environment variable is not set"))?
        };

        Ok(Self {
            config,
            platform,
            log,
            dry_run,
            home: std::path::PathBuf::from(home),
        })
    }

    /// Root directory of the dotfiles repository.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.config.root
    }

    /// Symlinks source directory.
    #[must_use]
    pub fn symlinks_dir(&self) -> std::path::PathBuf {
        self.config.root.join("symlinks")
    }

    /// Hooks source directory.
    #[must_use]
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

/// Counters for batch tasks that process many items.
///
/// Provides consistent summary logging across all tasks.
#[derive(Default)]
pub struct TaskStats {
    pub changed: u32,
    pub already_ok: u32,
    pub skipped: u32,
}

impl TaskStats {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Format the summary string (e.g. "3 changed, 10 already ok, 1 skipped").
    #[must_use]
    pub fn summary(&self, dry_run: bool) -> String {
        let verb = if dry_run { "would change" } else { "changed" };
        if self.skipped > 0 {
            format!(
                "{} {verb}, {} already ok, {} skipped",
                self.changed, self.already_ok, self.skipped
            )
        } else {
            format!("{} {verb}, {} already ok", self.changed, self.already_ok)
        }
    }

    /// Log the summary and return the appropriate `TaskResult`.
    #[must_use]
    pub fn finish(self, ctx: &Context) -> TaskResult {
        ctx.log.info(&self.summary(ctx.dry_run));
        if ctx.dry_run {
            TaskResult::DryRun
        } else {
            TaskResult::Ok
        }
    }
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
            .debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log
            .record_task(task.name(), TaskStatus::NotApplicable, None);
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
