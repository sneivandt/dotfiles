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
use crate::exec::Executor;
use crate::logging::{Logger, TaskStatus};
use crate::platform::Platform;
use crate::resources::{Resource, ResourceChange, ResourceState};

/// Shared context for task execution.
pub struct Context<'a> {
    /// Configuration loaded from INI files.
    pub config: &'a Config,
    /// Detected platform information.
    pub platform: &'a Platform,
    /// Logger for output and task recording.
    pub log: &'a Logger,
    /// Whether to perform a dry run (preview changes without applying).
    pub dry_run: bool,
    /// User's home directory path.
    pub home: std::path::PathBuf,
    /// Command executor (for testing or real system calls).
    pub executor: &'a dyn Executor,
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("config", &"<Config>")
            .field("platform", &self.platform)
            .field("log", &"<Logger>")
            .field("dry_run", &self.dry_run)
            .field("home", &self.home)
            .field("executor", &"<dyn Executor>")
            .finish()
    }
}

impl<'a> Context<'a> {
    /// Creates a new context for task execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the HOME (or USERPROFILE on Windows) environment variable
    /// is not set.
    pub fn new(
        config: &'a Config,
        platform: &'a Platform,
        log: &'a Logger,
        dry_run: bool,
        executor: &'a dyn Executor,
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
            executor,
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Default)]
pub struct TaskStats {
    /// Number of items changed or applied.
    pub changed: u32,
    /// Number of items already in the correct state.
    pub already_ok: u32,
    /// Number of items skipped due to errors or inapplicability.
    pub skipped: u32,
}

impl TaskStats {
    /// Create a new empty stats counter.
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
#[derive(Debug)]
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
///
/// # Errors
///
/// Returns an error if any resource fails to check its state or apply changes,
/// depending on the `bail_on_error` setting in `opts`. If `bail_on_error` is `false`,
/// errors are logged as warnings instead.
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
///
/// # Errors
///
/// Returns an error if any resource fails to apply changes, depending on the
/// `bail_on_error` setting in `opts`. If `bail_on_error` is `false`, errors are
/// logged as warnings instead.
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
///
/// # Errors
///
/// Returns an error if a resource fails to check its current state or fails
/// during the removal process.
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
        match resource.apply()? {
            ResourceChange::Applied => {
                ctx.log
                    .debug(&format!("{}: {}", opts.verb, resource.description()));
                counters.changed += 1;
            }
            ResourceChange::AlreadyCorrect => {
                counters.already_ok += 1;
            }
            ResourceChange::Skipped { reason } => {
                anyhow::bail!(
                    "failed to {} {}: {reason}",
                    opts.verb,
                    resource.description()
                );
            }
        }
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
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute, such as when system commands
    /// fail, file operations are not permitted, or configuration is invalid.
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

/// Shared helpers for task unit tests.
///
/// Provides common mock types and factory functions so each task test module
/// does not have to duplicate boilerplate.
#[cfg(test)]
pub(crate) mod test_helpers {
    use crate::config::Config;
    use crate::config::manifest::Manifest;
    use crate::config::profiles::Profile;
    use crate::exec::{ExecResult, Executor};
    use crate::logging::Logger;
    use crate::platform::Platform;
    use std::path::{Path, PathBuf};

    use super::Context;

    /// Minimal executor that panics if any real command is issued.
    ///
    /// `which()` always returns `false`, which causes tasks that guard on tool
    /// availability to report *not applicable*.  Use [`WhichExecutor`] when you
    /// need `which()` to return `true`.
    #[derive(Debug)]
    pub struct NoOpExecutor;

    impl Executor for NoOpExecutor {
        fn run(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn run_in(&self, _: &Path, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn run_unchecked(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn which(&self, _: &str) -> bool {
            false
        }
    }

    /// Executor that returns a fixed value for `which()` and panics for real calls.
    #[derive(Debug)]
    pub struct WhichExecutor {
        /// Value returned by `which()` regardless of program name.
        pub which_result: bool,
    }

    impl Executor for WhichExecutor {
        fn run(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn run_in(&self, _: &Path, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn run_unchecked(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            panic!("unexpected executor call in test")
        }

        fn which(&self, _: &str) -> bool {
            self.which_result
        }
    }

    /// Build a [`Config`] with all lists empty and `root` set to `root`.
    #[allow(clippy::expect_used)]
    pub fn empty_config(root: PathBuf) -> Config {
        Config {
            root,
            profile: Profile {
                name: "test".to_string(),
                active_categories: vec!["base".to_string()],
                excluded_categories: vec![],
            },
            packages: vec![],
            symlinks: vec![],
            registry: vec![],
            units: vec![],
            chmod: vec![],
            vscode_extensions: vec![],
            copilot_skills: vec![],
            manifest: Manifest {
                excluded_files: vec![],
            },
        }
    }

    /// Build a [`Context`] from the given config, platform and executor.
    ///
    /// The logger is leaked intentionally: test contexts are short-lived and
    /// leaking is harmless in a test binary.
    pub fn make_context<'a>(
        config: &'a Config,
        platform: &'a Platform,
        executor: &'a dyn Executor,
    ) -> Context<'a> {
        Context {
            config,
            platform,
            log: Box::leak(Box::new(Logger::new(false, "test"))),
            dry_run: false,
            home: PathBuf::from("/home/test"),
            executor,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::config::profiles::Profile;
    use crate::exec::{ExecResult, Executor};
    use crate::platform::{Os, Platform};
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Test doubles
    // -----------------------------------------------------------------------

    /// A configurable mock resource for testing the processing pipeline.
    struct MockResource {
        state: ResourceState,
        apply_result: Result<ResourceChange, String>,
        remove_result: Result<ResourceChange, String>,
        desc: String,
    }

    impl MockResource {
        fn new(state: ResourceState) -> Self {
            Self {
                state,
                apply_result: Ok(ResourceChange::Applied),
                remove_result: Ok(ResourceChange::Applied),
                desc: "mock resource".to_string(),
            }
        }

        fn with_apply(mut self, result: Result<ResourceChange, String>) -> Self {
            self.apply_result = result;
            self
        }

        fn with_remove(mut self, result: Result<ResourceChange, String>) -> Self {
            self.remove_result = result;
            self
        }

        #[allow(dead_code)]
        fn with_desc(mut self, desc: &str) -> Self {
            self.desc = desc.to_string();
            self
        }
    }

    impl Resource for MockResource {
        fn description(&self) -> String {
            self.desc.clone()
        }

        fn current_state(&self) -> Result<ResourceState> {
            Ok(self.state.clone())
        }

        fn apply(&self) -> Result<ResourceChange> {
            self.apply_result
                .clone()
                .map_err(|s| anyhow::anyhow!("{s}"))
        }

        fn remove(&self) -> Result<ResourceChange> {
            self.remove_result
                .clone()
                .map_err(|s| anyhow::anyhow!("{s}"))
        }
    }

    /// Minimal executor that panics if actually called (tests don't need it).
    #[derive(Debug)]
    struct NoOpExecutor;

    impl Executor for NoOpExecutor {
        fn run(&self, _program: &str, _args: &[&str]) -> Result<ExecResult> {
            panic!("unexpected executor call in test");
        }
        fn run_in(&self, _dir: &Path, _program: &str, _args: &[&str]) -> Result<ExecResult> {
            panic!("unexpected executor call in test");
        }
        fn run_in_with_env(
            &self,
            _dir: &Path,
            _program: &str,
            _args: &[&str],
            _env: &[(&str, &str)],
        ) -> Result<ExecResult> {
            panic!("unexpected executor call in test");
        }
        fn run_unchecked(&self, _program: &str, _args: &[&str]) -> Result<ExecResult> {
            panic!("unexpected executor call in test");
        }
        fn which(&self, _program: &str) -> bool {
            false
        }
    }

    /// A mock task for testing `execute()`.
    struct MockTask {
        name: &'static str,
        should_run: bool,
        result: Result<TaskResult, String>,
    }

    impl Task for MockTask {
        fn name(&self) -> &str {
            self.name
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            self.should_run
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.result.clone().map_err(|s| anyhow::anyhow!("{s}"))
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn empty_config(root: PathBuf) -> Config {
        Config {
            root,
            profile: Profile {
                name: "test".to_string(),
                active_categories: vec!["base".to_string()],
                excluded_categories: vec![],
            },
            packages: vec![],
            symlinks: vec![],
            registry: vec![],
            units: vec![],
            chmod: vec![],
            vscode_extensions: vec![],
            copilot_skills: vec![],
            manifest: crate::config::manifest::Manifest {
                excluded_files: vec![],
            },
        }
    }

    fn test_context(config: &Config) -> Context<'_> {
        let platform = Box::leak(Box::new(Platform::new(Os::Linux, false)));
        let log = Box::leak(Box::new(Logger::new(false, "test")));
        let executor = Box::leak(Box::new(NoOpExecutor));
        Context {
            config,
            platform,
            log,
            dry_run: false,
            home: PathBuf::from("/home/test"),
            executor,
        }
    }

    fn dry_run_context(config: &Config) -> Context<'_> {
        let mut ctx = test_context(config);
        ctx.dry_run = true;
        ctx
    }

    fn default_opts() -> ProcessOpts<'static> {
        ProcessOpts {
            verb: "install",
            fix_incorrect: true,
            fix_missing: true,
            bail_on_error: false,
        }
    }

    fn bail_opts() -> ProcessOpts<'static> {
        ProcessOpts {
            verb: "install",
            fix_incorrect: true,
            fix_missing: true,
            bail_on_error: true,
        }
    }

    // -----------------------------------------------------------------------
    // TaskStats
    // -----------------------------------------------------------------------

    #[test]
    fn stats_summary_changed_only() {
        let stats = TaskStats {
            changed: 3,
            already_ok: 0,
            skipped: 0,
        };
        assert_eq!(stats.summary(false), "3 changed, 0 already ok");
    }

    #[test]
    fn stats_summary_dry_run() {
        let stats = TaskStats {
            changed: 2,
            already_ok: 5,
            skipped: 0,
        };
        assert_eq!(stats.summary(true), "2 would change, 5 already ok");
    }

    #[test]
    fn stats_summary_with_skipped() {
        let stats = TaskStats {
            changed: 1,
            already_ok: 2,
            skipped: 3,
        };
        assert_eq!(stats.summary(false), "1 changed, 2 already ok, 3 skipped");
    }

    #[test]
    fn stats_finish_returns_dry_run_result() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = dry_run_context(&config);
        let stats = TaskStats::new();
        let result = stats.finish(&ctx);
        assert!(matches!(result, TaskResult::DryRun));
    }

    #[test]
    fn stats_finish_returns_ok_result() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let stats = TaskStats::new();
        let result = stats.finish(&ctx);
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // process_single
    // -----------------------------------------------------------------------

    #[test]
    fn process_single_correct_increments_already_ok() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Correct);
        let opts = default_opts();
        let mut stats = TaskStats::new();

        process_single(&ctx, &resource, ResourceState::Correct, &opts, &mut stats).unwrap();

        assert_eq!(stats.already_ok, 1);
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn process_single_invalid_increments_skipped() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Invalid {
            reason: "test".to_string(),
        });
        let opts = default_opts();
        let mut stats = TaskStats::new();

        process_single(
            &ctx,
            &resource,
            ResourceState::Invalid {
                reason: "test".to_string(),
            },
            &opts,
            &mut stats,
        )
        .unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn process_single_missing_skips_when_fix_missing_false() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = ProcessOpts {
            fix_missing: false,
            ..default_opts()
        };
        let mut stats = TaskStats::new();

        process_single(&ctx, &resource, ResourceState::Missing, &opts, &mut stats).unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn process_single_incorrect_skips_when_fix_incorrect_false() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Incorrect {
            current: "wrong".to_string(),
        });
        let opts = ProcessOpts {
            fix_incorrect: false,
            ..default_opts()
        };
        let mut stats = TaskStats::new();

        process_single(
            &ctx,
            &resource,
            ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
            &opts,
            &mut stats,
        )
        .unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn process_single_missing_applies_and_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = default_opts();
        let mut stats = TaskStats::new();

        process_single(&ctx, &resource, ResourceState::Missing, &opts, &mut stats).unwrap();

        assert_eq!(stats.changed, 1);
        assert_eq!(stats.already_ok, 0);
    }

    #[test]
    fn process_single_incorrect_applies_and_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Incorrect {
            current: "wrong".to_string(),
        });
        let opts = default_opts();
        let mut stats = TaskStats::new();

        process_single(
            &ctx,
            &resource,
            ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
            &opts,
            &mut stats,
        )
        .unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn process_single_dry_run_missing_increments_changed_without_apply() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = dry_run_context(&config);
        // Apply would error if called — but dry-run should skip it
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Err("should not call".into()));
        let opts = default_opts();
        let mut stats = TaskStats::new();

        process_single(&ctx, &resource, ResourceState::Missing, &opts, &mut stats).unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn process_single_dry_run_incorrect_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = dry_run_context(&config);
        let resource = MockResource::new(ResourceState::Incorrect {
            current: "old-value".to_string(),
        });
        let opts = default_opts();
        let mut stats = TaskStats::new();

        process_single(
            &ctx,
            &resource,
            ResourceState::Incorrect {
                current: "old-value".to_string(),
            },
            &opts,
            &mut stats,
        )
        .unwrap();

        assert_eq!(stats.changed, 1);
    }

    // -----------------------------------------------------------------------
    // apply_resource
    // -----------------------------------------------------------------------

    #[test]
    fn apply_resource_applied_increments_changed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = default_opts();
        let mut stats = TaskStats::new();

        apply_resource(&ctx, &resource, &opts, &mut stats).unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn apply_resource_already_correct_increments_already_ok() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Missing)
            .with_apply(Ok(ResourceChange::AlreadyCorrect));
        let opts = default_opts();
        let mut stats = TaskStats::new();

        apply_resource(&ctx, &resource, &opts, &mut stats).unwrap();

        assert_eq!(stats.already_ok, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn apply_resource_skipped_no_bail_increments_skipped() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::Skipped {
                reason: "not supported".to_string(),
            }));
        let opts = default_opts();
        let mut stats = TaskStats::new();

        apply_resource(&ctx, &resource, &opts, &mut stats).unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn apply_resource_error_no_bail_increments_skipped() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Err("boom".to_string()));
        let opts = default_opts();
        let mut stats = TaskStats::new();

        apply_resource(&ctx, &resource, &opts, &mut stats).unwrap();

        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.changed, 0);
    }

    #[test]
    fn apply_resource_bail_on_applied() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Missing);
        let opts = bail_opts();
        let mut stats = TaskStats::new();

        apply_resource(&ctx, &resource, &opts, &mut stats).unwrap();

        assert_eq!(stats.changed, 1);
    }

    #[test]
    fn apply_resource_bail_on_already_correct() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource = MockResource::new(ResourceState::Missing)
            .with_apply(Ok(ResourceChange::AlreadyCorrect));
        let opts = bail_opts();
        let mut stats = TaskStats::new();

        apply_resource(&ctx, &resource, &opts, &mut stats).unwrap();

        assert_eq!(stats.already_ok, 1);
    }

    #[test]
    fn apply_resource_bail_on_skipped_returns_error() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Ok(ResourceChange::Skipped {
                reason: "denied".to_string(),
            }));
        let opts = bail_opts();
        let mut stats = TaskStats::new();

        let err = apply_resource(&ctx, &resource, &opts, &mut stats);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("denied"));
    }

    #[test]
    fn apply_resource_bail_on_error_propagates() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource =
            MockResource::new(ResourceState::Missing).with_apply(Err("critical".to_string()));
        let opts = bail_opts();
        let mut stats = TaskStats::new();

        let err = apply_resource(&ctx, &resource, &opts, &mut stats);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("critical"));
    }

    // -----------------------------------------------------------------------
    // process_resources
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_mixed_states() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resources = vec![
            MockResource::new(ResourceState::Correct),
            MockResource::new(ResourceState::Missing),
            MockResource::new(ResourceState::Invalid {
                reason: "bad".to_string(),
            }),
        ];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn process_resources_empty_list() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resources: Vec<MockResource> = vec![];
        let opts = default_opts();

        let result = process_resources(&ctx, resources, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // process_resource_states
    // -----------------------------------------------------------------------

    #[test]
    fn process_resource_states_applies_precomputed() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resource_states = vec![
            (
                MockResource::new(ResourceState::Missing),
                ResourceState::Missing,
            ),
            (
                MockResource::new(ResourceState::Correct),
                ResourceState::Correct,
            ),
        ];
        let opts = default_opts();

        let result = process_resource_states(&ctx, resource_states, &opts).unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    // -----------------------------------------------------------------------
    // process_resources_remove
    // -----------------------------------------------------------------------

    #[test]
    fn process_resources_remove_removes_correct_resources() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let resources = vec![
            MockResource::new(ResourceState::Correct),
            MockResource::new(ResourceState::Missing),
        ];

        let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
        assert!(matches!(result, TaskResult::Ok));
    }

    #[test]
    fn process_resources_remove_dry_run() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = dry_run_context(&config);
        // Remove should NOT be called in dry-run
        let resources = vec![
            MockResource::new(ResourceState::Correct).with_remove(Err("should not call".into())),
        ];

        let result = process_resources_remove(&ctx, resources, "unlink").unwrap();
        assert!(matches!(result, TaskResult::DryRun));
    }

    // -----------------------------------------------------------------------
    // execute
    // -----------------------------------------------------------------------

    #[test]
    fn execute_skips_non_applicable_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let task = MockTask {
            name: "test-task",
            should_run: false,
            result: Ok(TaskResult::Ok),
        };

        execute(&task, &ctx);
        assert_eq!(ctx.log.failure_count(), 0);
    }

    #[test]
    fn execute_records_ok_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let task = MockTask {
            name: "ok-task",
            should_run: true,
            result: Ok(TaskResult::Ok),
        };

        execute(&task, &ctx);
        assert_eq!(ctx.log.failure_count(), 0);
    }

    #[test]
    fn execute_records_failed_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let task = MockTask {
            name: "fail-task",
            should_run: true,
            result: Err("kaboom".to_string()),
        };

        execute(&task, &ctx);
        assert_eq!(ctx.log.failure_count(), 1);
    }

    #[test]
    fn execute_records_skipped_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let task = MockTask {
            name: "skip-task",
            should_run: true,
            result: Ok(TaskResult::Skipped("not needed".to_string())),
        };

        execute(&task, &ctx);
        assert_eq!(ctx.log.failure_count(), 0);
    }

    #[test]
    fn execute_records_dry_run_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let ctx = test_context(&config);
        let task = MockTask {
            name: "dry-task",
            should_run: true,
            result: Ok(TaskResult::DryRun),
        };

        execute(&task, &ctx);
        assert_eq!(ctx.log.failure_count(), 0);
    }
}
