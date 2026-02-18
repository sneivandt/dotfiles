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
use crate::resources::{Resource, ResourceChange, ResourceState};

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

/// Configuration for the generic resource processing loop.
///
/// Controls how each [`ResourceState`] variant is handled.
pub struct ProcessOpts<'a> {
    /// Verb for log messages (e.g., "install", "link", "chmod").
    pub verb: &'a str,
    /// Treat `Incorrect` as fixable (apply the change). If `false`, skip it.
    pub fix_incorrect: bool,
    /// Treat `Missing` as fixable (apply the change). If `false`, skip it.
    pub fix_missing: bool,
    /// Propagate errors from `apply()` (bail). If `false`, warn and count as skipped.
    pub bail_on_error: bool,
}

/// Process resources by checking each one's current state and applying as needed.
///
/// For tasks where each resource can independently determine its own state via
/// `resource.current_state()`.
pub fn process_resources<R: Resource>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    let mut stats = TaskStats::new();
    for resource in resources {
        let current = resource.current_state()?;
        process_single(ctx, &resource, current, opts, &mut stats)?;
    }
    Ok(stats.finish(ctx))
}

/// Process resources with pre-computed states.
///
/// For tasks that batch-query state (e.g., registry, packages, VS Code extensions)
/// and then iterate with cached results.
pub fn process_resource_states<R: Resource>(
    ctx: &Context,
    resource_states: impl IntoIterator<Item = (R, ResourceState)>,
    opts: &ProcessOpts,
) -> Result<TaskResult> {
    let mut stats = TaskStats::new();
    for (resource, current) in resource_states {
        process_single(ctx, &resource, current, opts, &mut stats)?;
    }
    Ok(stats.finish(ctx))
}

/// Process resources for removal.
///
/// Only resources in [`ResourceState::Correct`] are removed (they are "ours").
/// Resources that are `Missing`, `Incorrect`, or `Invalid` are skipped.
pub fn process_resources_remove<R: Resource>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    verb: &str,
) -> Result<TaskResult> {
    let mut stats = TaskStats::new();
    for resource in resources {
        let current = resource.current_state()?;
        match current {
            ResourceState::Correct => {
                if ctx.dry_run {
                    ctx.log
                        .dry_run(&format!("would {verb}: {}", resource.description()));
                    stats.changed += 1;
                    continue;
                }
                resource.remove()?;
                ctx.log
                    .debug(&format!("{verb}: {}", resource.description()));
                stats.changed += 1;
            }
            _ => {
                // Not ours or doesn't exist — skip silently
                stats.already_ok += 1;
            }
        }
    }
    Ok(stats.finish(ctx))
}

/// Process a single resource given its current state.
fn process_single<R: Resource>(
    ctx: &Context,
    resource: &R,
    resource_state: ResourceState,
    opts: &ProcessOpts,
    counters: &mut TaskStats,
) -> Result<()> {
    match resource_state {
        ResourceState::Correct => {
            ctx.log.debug(&format!("ok: {}", resource.description()));
            counters.already_ok += 1;
        }
        ResourceState::Invalid { reason } => {
            ctx.log
                .debug(&format!("skipping {}: {reason}", resource.description()));
            counters.skipped += 1;
        }
        ResourceState::Missing if !opts.fix_missing => {
            counters.skipped += 1;
        }
        ResourceState::Incorrect { .. } if !opts.fix_incorrect => {
            ctx.log.debug(&format!(
                "skipping {} (unexpected state)",
                resource.description()
            ));
            counters.skipped += 1;
        }
        resource_state @ (ResourceState::Missing | ResourceState::Incorrect { .. }) => {
            if ctx.dry_run {
                let msg = if let ResourceState::Incorrect { ref current } = resource_state {
                    format!(
                        "would {} {} (currently {current})",
                        opts.verb,
                        resource.description()
                    )
                } else {
                    format!("would {}: {}", opts.verb, resource.description())
                };
                ctx.log.dry_run(&msg);
                counters.changed += 1;
                return Ok(());
            }
            apply_resource(ctx, resource, opts, counters)?;
        }
    }
    Ok(())
}

/// Apply a single resource change, handling errors per [`ProcessOpts`].
fn apply_resource<R: Resource>(
    ctx: &Context,
    resource: &R,
    opts: &ProcessOpts,
    counters: &mut TaskStats,
) -> Result<()> {
    if opts.bail_on_error {
        resource.apply()?;
        ctx.log
            .debug(&format!("{}: {}", opts.verb, resource.description()));
        counters.changed += 1;
    } else {
        match resource.apply() {
            Ok(ResourceChange::Applied) => {
                ctx.log
                    .debug(&format!("{}: {}", opts.verb, resource.description()));
                counters.changed += 1;
            }
            Ok(ResourceChange::Skipped { reason }) => {
                ctx.log.warn(&format!(
                    "failed to {} {}: {reason}",
                    opts.verb,
                    resource.description()
                ));
                counters.skipped += 1;
            }
            Ok(ResourceChange::AlreadyCorrect) => {
                counters.already_ok += 1;
            }
            Err(e) => {
                ctx.log.warn(&format!(
                    "failed to {} {}: {e}",
                    opts.verb,
                    resource.description()
                ));
                counters.skipped += 1;
            }
        }
    }
    Ok(())
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
