//! Named, dependency-ordered tasks that orchestrate resource changes.
//!
//! Phases are organised into three sub-modules:
//!
//! - **Bootstrap** (`phases::bootstrap`) — prepare the dotfiles tool itself
//!   (binary update, wrapper installation, PATH configuration).
//! - **Repository** (`phases::repository`) — synchronise the dotfiles repository
//!   (sparse checkout, pull, config reload, hooks).
//! - **Apply** (`phases::apply`) — apply declared state to the user environment
//!   (symlinks, packages, registry, etc.).

pub mod apply;
pub mod bootstrap;
mod catalog;
mod macros;
pub mod repository;
pub mod validation;

pub use catalog::{all_install_tasks, all_uninstall_tasks};
pub(crate) use macros::{batch_resource_task, resource_task, task_deps};

// Re-export engine types so downstream `use super::` and `use crate::phases::`
// continue to work unchanged.
pub use crate::engine::Context;
pub use crate::engine::ContextOpts;
pub(crate) use crate::engine::graph::has_cycle;
pub use crate::engine::update_signal::UpdateSignal;
#[allow(unused_imports)] // TaskStats is used by doc-tests via the lib crate
pub use crate::engine::{
    ProcessMode, ProcessOpts, ResourceAction, TaskResult, TaskStats, process_resource_states,
    process_resources, process_resources_remove,
};

use std::any::TypeId;
use std::fmt;

use anyhow::Result;

use crate::logging::TaskStatus;

/// Execution phase of a task.
///
/// Bootstrap tasks run first to prepare the tool itself (binary update,
/// wrapper installation, PATH configuration).  Repository tasks run
/// second to synchronise the dotfiles repository (sparse checkout,
/// pull, config reload, hooks).  Apply tasks run last to converge the
/// user environment to its declared state (symlinks, packages, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskPhase {
    /// Prepare the dotfiles tool itself.
    Bootstrap,
    /// Synchronise the dotfiles repository.
    Repository,
    /// Apply declared state to the user environment.
    Apply,
}

impl fmt::Display for TaskPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap => f.write_str("Bootstrap"),
            Self::Repository => f.write_str("Repository"),
            Self::Apply => f.write_str("Apply"),
        }
    }
}

/// A named, executable task.
///
/// The `'static` bound is required so that each task struct has a stable
/// [`TypeId`] which the scheduler uses to match dependency declarations
/// (see [`Task::task_id`] and [`Task::dependencies`]).
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Execution phase: [`TaskPhase::Bootstrap`], [`TaskPhase::Repository`], or [`TaskPhase::Apply`].
    fn phase(&self) -> TaskPhase;

    /// The concrete `TypeId` of this task, used as a dependency identifier.
    ///
    /// The default implementation uses `TypeId::of::<Self>()` which is correct
    /// for all concrete (non-generic) task structs.
    fn task_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Tasks that must complete before this task starts.
    ///
    /// Return `TypeId`s of the concrete task structs that this task depends on.
    /// The scheduler uses this information to build a dependency graph and
    /// execute independent tasks in parallel.  The default implementation
    /// returns an empty slice (no dependencies).
    ///
    /// Use `TypeId::of::<TaskStruct>()` to reference a dependency.
    fn dependencies(&self) -> &[TypeId] {
        &[]
    }

    /// Whether this task should run on the current platform/profile.
    fn should_run(&self, ctx: &Context) -> bool;

    /// Execute the task when it is applicable, combining the applicability
    /// check and run step into a single call.
    ///
    /// Returning `Ok(None)` means the task is not applicable and should be
    /// recorded as such without treating the task as a failure. The default
    /// implementation emits a stage header and delegates to [`Task::run`];
    /// macros can override it to emit the stage header only when items are
    /// present, avoiding `==>` output for tasks with nothing configured.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute.
    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        ctx.log.stage(self.name());
        self.run(ctx).map(Some)
    }

    /// Whether this task will need sudo/root privileges based on current state.
    ///
    /// Called before parallel dispatch to allow the runner to prime the
    /// credential cache (`sudo -v`) so that interactive prompts do not
    /// collide with parallel output.  The default returns `false`.
    fn needs_sudo(&self, _ctx: &Context) -> bool {
        false
    }

    /// Execute the task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute, such as when system commands
    /// fail, file operations are not permitted, or configuration is invalid.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}

/// Record and optionally emit a compact task-result line.
fn record(ctx: &Context, name: &str, phase: TaskPhase, status: TaskStatus, msg: Option<&str>) {
    ctx.log.record_task(name, phase, status, msg);
    if !ctx.log.is_verbose() {
        ctx.log.emit_task_result(name, &status, msg);
    }
}

/// Execute a task, recording the result in the logger.
///
/// Each task invocation is wrapped in a [`tracing::info_span`] so that
/// the log file and diagnostic output include structured context about
/// which task produced each message.
///
/// If cancellation has been requested (Ctrl-C) and a task returns
/// [`TaskResult::Failed`] or an error, the failure is downgraded to
/// [`TaskStatus::Skipped`] with an "interrupted" message so the
/// summary does not count signal-induced failures.
pub fn execute(task: &dyn Task, ctx: &Context) {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();
    let phase = task.phase();

    if !task.should_run(ctx) {
        if ctx.log.debug_enabled() {
            ctx.log.stage(task.name());
        }
        ctx.log
            .debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log
            .record_task(task.name(), phase, TaskStatus::NotApplicable, None);
        return;
    }

    match task.run_if_applicable(ctx) {
        Ok(None) => {
            if ctx.log.debug_enabled() {
                ctx.log.stage(task.name());
            }
            ctx.log.debug("nothing configured");
            ctx.log
                .record_task(task.name(), phase, TaskStatus::NotApplicable, None);
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => record(ctx, task.name(), phase, TaskStatus::Ok, None),
            TaskResult::NotApplicable(reason) => {
                ctx.debug_fmt(|| format!("not applicable: {reason}"));
                ctx.log
                    .record_task(task.name(), phase, TaskStatus::NotApplicable, None);
            }
            TaskResult::Skipped(reason) => {
                ctx.log.info(&format!("skipped: {reason}"));
                record(ctx, task.name(), phase, TaskStatus::Skipped, Some(&reason));
            }
            TaskResult::Failed(reason) => {
                if ctx.is_cancelled() {
                    ctx.log.warn(&format!("interrupted: {reason}"));
                    record(
                        ctx,
                        task.name(),
                        phase,
                        TaskStatus::Skipped,
                        Some("interrupted"),
                    );
                } else {
                    ctx.log.warn(&format!("failed: {reason}"));
                    record(ctx, task.name(), phase, TaskStatus::Failed, Some(&reason));
                }
            }
            TaskResult::DryRun => record(ctx, task.name(), phase, TaskStatus::DryRun, None),
        },
        Err(e) => {
            if ctx.is_cancelled() {
                ctx.log.warn(&format!("interrupted: {}", task.name()));
                record(
                    ctx,
                    task.name(),
                    phase,
                    TaskStatus::Skipped,
                    Some("interrupted"),
                );
            } else {
                ctx.log.error(&format!("{}: {e:#}", task.name()));
                record(
                    ctx,
                    task.name(),
                    phase,
                    TaskStatus::Failed,
                    Some(&format!("{e:#}")),
                );
            }
        }
    }
}

/// Shared helpers for task unit tests.
///
/// Provides common mock types and factory functions so each task test module
/// does not have to duplicate boilerplate.
#[cfg(test)]
#[allow(clippy::panic)]
pub mod test_helpers {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::config::Config;
    use crate::config::category_matcher::Category;
    use crate::config::manifest::Manifest;
    use crate::config::profiles::Profile;
    use crate::exec::{Executor, MockExecutor};
    use crate::logging::Logger;
    use crate::platform::Platform;

    use super::Context;

    /// Build a [`Config`] with all lists empty and `root` set to `root`.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn empty_config(root: PathBuf) -> Config {
        Config {
            root,
            overlay: None,
            profile: Profile {
                name: "test".to_string(),
                active_categories: vec![Category::Base],
                excluded_categories: vec![],
            },
            packages: vec![],
            symlinks: vec![],
            registry: vec![],
            units: vec![],
            chmod: vec![],
            vscode_extensions: vec![],
            copilot_plugins: vec![],
            git_settings: vec![],
            manifest: Manifest {
                excluded_files: vec![],
            },
            scripts: vec![],
        }
    }

    /// Build a [`Context`] from the given config, platform and executor.
    pub fn make_context(
        config: Config,
        platform: Platform,
        executor: Arc<dyn Executor>,
    ) -> Context {
        Context::from_raw(
            Arc::new(std::sync::RwLock::new(Arc::new(config))),
            platform,
            Arc::new(Logger::new("test")),
            executor,
            PathBuf::from("/home/test"),
            crate::engine::ContextOpts {
                dry_run: false,
                parallel: false,
                is_ci: Some(false),
            },
        )
    }

    /// Build a stub [`MockExecutor`] that returns `which_result` for every
    /// `which()` / `which_path()` call and panics on any `run*()` call.
    #[must_use]
    pub fn stub_executor(which_result: bool) -> MockExecutor {
        let mut mock = MockExecutor::new();
        mock.expect_which().returning(move |_| which_result);
        mock.expect_which_path().returning(move |program| {
            if which_result {
                #[cfg(windows)]
                let path = PathBuf::from(format!(r"C:\Windows\System32\{program}.exe"));
                #[cfg(not(windows))]
                let path = PathBuf::from(format!("/usr/bin/{program}"));
                Ok(path)
            } else {
                anyhow::bail!("{program} not found on PATH")
            }
        });
        mock
    }

    /// Builder for test [`Context`] instances.
    ///
    /// Provides a fluent API so that tests can construct exactly the context
    /// variant they need without relying on a growing list of factory functions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let ctx = ContextBuilder::new(config)
    ///     .os(crate::platform::Os::Linux)
    ///     .arch(true)
    ///     .which(true)
    ///     .build();
    /// ```
    #[derive(Debug)]
    #[must_use]
    #[allow(clippy::struct_excessive_bools)]
    pub struct ContextBuilder {
        config: Config,
        os: crate::platform::Os,
        is_arch: bool,
        is_wsl: bool,
        which_result: bool,
        is_ci: bool,
    }

    impl ContextBuilder {
        /// Create a new builder with Linux, non-arch, `which = false` defaults.
        pub fn new(config: Config) -> Self {
            Self {
                config,
                os: crate::platform::Os::Linux,
                is_arch: false,
                is_wsl: false,
                which_result: false,
                is_ci: false,
            }
        }

        /// Set the target OS.
        pub fn os(mut self, os: crate::platform::Os) -> Self {
            self.os = os;
            self
        }

        /// Set whether the platform is Arch Linux.
        pub fn arch(mut self, is_arch: bool) -> Self {
            self.is_arch = is_arch;
            self
        }

        /// Set whether the platform is Windows Subsystem for Linux.
        pub fn wsl(mut self, is_wsl: bool) -> Self {
            self.is_wsl = is_wsl;
            self
        }

        /// Set the value returned by `executor.which()`.
        pub fn which(mut self, which_result: bool) -> Self {
            self.which_result = which_result;
            self
        }

        /// Set whether the context simulates a CI environment.
        ///
        /// Tasks that check [`Context::is_ci`] (such as `ConfigureShell`)
        /// can be tested without mutating process-global environment variables.
        pub fn ci(mut self, is_ci: bool) -> Self {
            self.is_ci = is_ci;
            self
        }

        /// Consume the builder and produce a [`Context`].
        #[must_use]
        pub fn build(self) -> Context {
            make_context(
                self.config,
                Platform {
                    os: self.os,
                    is_arch: self.is_arch,
                    is_wsl: self.is_wsl,
                },
                Arc::new(stub_executor(self.which_result)),
            )
            .with_ci(self.is_ci)
        }
    }

    /// Build a [`Context`] with the specified OS/arch and default [`MockExecutor`].
    #[must_use]
    pub fn make_platform_context(
        config: Config,
        os: crate::platform::Os,
        is_arch: bool,
    ) -> Context {
        ContextBuilder::new(config).os(os).arch(is_arch).build()
    }

    /// Build a [`Context`] with the specified OS/arch and a [`MockExecutor`]
    /// that returns the given `which_result`.
    ///
    /// Use this when a task's `should_run` or `run` method gates on tool
    /// availability via `ctx.executor.which(...)`.
    #[must_use]
    pub fn make_platform_context_with_which(
        config: Config,
        os: crate::platform::Os,
        is_arch: bool,
        which_result: bool,
    ) -> Context {
        ContextBuilder::new(config)
            .os(os)
            .arch(is_arch)
            .which(which_result)
            .build()
    }

    /// Build a [`Context`] with a Linux non-arch platform and default [`MockExecutor`].
    ///
    /// Convenience shorthand for tests that only need a plain Linux context.
    #[must_use]
    pub fn make_linux_context(config: Config) -> Context {
        ContextBuilder::new(config).build()
    }

    /// Build a [`Context`] with a Windows platform and default [`MockExecutor`].
    ///
    /// Convenience shorthand for tests that only need a plain Windows context.
    #[must_use]
    pub fn make_windows_context(config: Config) -> Context {
        ContextBuilder::new(config)
            .os(crate::platform::Os::Windows)
            .build()
    }

    /// Build a [`Context`] with an Arch Linux platform and default [`MockExecutor`].
    ///
    /// Convenience shorthand for tests that target Arch-specific behaviour.
    #[must_use]
    pub fn make_arch_context(config: Config) -> Context {
        ContextBuilder::new(config).arch(true).build()
    }

    /// Build a [`Context`] with a default Linux platform and
    /// default [`MockExecutor`], also returning the [`Logger`] so tests can
    /// inspect recorded task state.
    #[must_use]
    pub fn make_static_context(config: Config) -> (Context, Arc<Logger>) {
        let log = Arc::new(Logger::new("test"));
        let ctx =
            make_linux_context(config).with_log(Arc::clone(&log) as Arc<dyn crate::logging::Log>);
        (ctx, log)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::resources::{Applicable, Resource, ResourceChange, ResourceState};
    use anyhow::Result;
    use std::cell::Cell;
    use std::path::PathBuf;
    use test_helpers::{empty_config, make_static_context};

    thread_local! {
        static RESOURCE_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
        static BATCH_TASK_ITEM_EVALS: Cell<usize> = const { Cell::new(0) };
    }

    #[derive(Debug)]
    struct DummyResource;

    impl Applicable for DummyResource {
        fn description(&self) -> String {
            "dummy".to_string()
        }

        fn apply(&self) -> Result<ResourceChange> {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }

    impl Resource for DummyResource {
        fn current_state(&self) -> Result<ResourceState> {
            Ok(ResourceState::Correct)
        }
    }

    resource_task! {
        /// Test-only task for resource-task macro behaviour.
        CountingResourceTask {
            name: "Counting resource task",
            phase: TaskPhase::Apply,
            items: |_ctx| {
                RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::<()>::new()
            },
            build: |_item, _ctx| DummyResource,
            opts: ProcessOpts::strict("count"),
        }
    }

    batch_resource_task! {
        /// Test-only task for batch-resource-task macro behaviour.
        CountingBatchTask {
            name: "Counting batch task",
            phase: TaskPhase::Apply,
            items: |_ctx| {
                BATCH_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::<()>::new()
            },
            cache: |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
            build: |_item, _ctx| DummyResource,
            state: |_resource, _cache| ResourceState::Correct,
            opts: ProcessOpts::strict("count"),
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
        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            self.should_run
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.result.clone().map_err(|s| anyhow::anyhow!("{s}"))
        }
    }

    #[test]
    fn execute_skips_non_applicable_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let task = MockTask {
            name: "test-task",
            should_run: false,
            result: Ok(TaskResult::Ok),
        };

        execute(&task, &ctx);
        assert_eq!(log.failure_count(), 0);
    }

    #[test]
    fn execute_records_ok_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let task = MockTask {
            name: "ok-task",
            should_run: true,
            result: Ok(TaskResult::Ok),
        };

        execute(&task, &ctx);
        assert_eq!(log.failure_count(), 0);
    }

    #[test]
    fn execute_records_failed_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let task = MockTask {
            name: "fail-task",
            should_run: true,
            result: Err("kaboom".to_string()),
        };

        execute(&task, &ctx);
        assert_eq!(log.failure_count(), 1);
    }

    #[test]
    fn execute_records_skipped_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let task = MockTask {
            name: "skip-task",
            should_run: true,
            result: Ok(TaskResult::Skipped("not needed".to_string())),
        };

        execute(&task, &ctx);
        assert_eq!(log.failure_count(), 0);
    }

    #[test]
    fn execute_records_task_result_failed_as_failure() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let task = MockTask {
            name: "failed-task",
            should_run: true,
            result: Ok(TaskResult::Failed("git pull failed".to_string())),
        };

        execute(&task, &ctx);
        assert_eq!(log.failure_count(), 1);
    }

    #[test]
    fn execute_records_dry_run_task() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let task = MockTask {
            name: "dry-task",
            should_run: true,
            result: Ok(TaskResult::DryRun),
        };

        execute(&task, &ctx);
        assert_eq!(log.failure_count(), 0);
    }

    #[test]
    fn resource_task_should_run_does_not_evaluate_items() {
        RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(0));
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);

        assert!(CountingResourceTask.should_run(&ctx));
        RESOURCE_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn resource_task_run_evaluates_items_once_when_called_directly() {
        RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(0));
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);

        let result = CountingResourceTask.run(&ctx).unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
        RESOURCE_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
    }

    #[test]
    fn batch_task_should_run_does_not_evaluate_items() {
        BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);

        assert!(CountingBatchTask.should_run(&ctx));
        BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn batch_task_run_if_applicable_evaluates_items_once() {
        BATCH_TASK_ITEM_EVALS.with(|count| count.set(0));
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);

        let result = CountingBatchTask.run_if_applicable(&ctx).unwrap();
        assert!(result.is_none());
        BATCH_TASK_ITEM_EVALS.with(|count| assert_eq!(count.get(), 1));
    }

    // ------------------------------------------------------------------
    // Task registration completeness
    // ------------------------------------------------------------------

    /// Guard against forgetting to register a new task.
    ///
    /// When you add a new task to the codebase, add it to
    /// `all_install_tasks()` and bump the expected count here.
    #[test]
    fn all_install_tasks_count() {
        let tasks = all_install_tasks();
        assert_eq!(
            tasks.len(),
            22,
            "expected 22 install tasks — did you add a new task without updating \
             all_install_tasks()? Update the registration list and this test."
        );
    }

    #[test]
    fn all_uninstall_tasks_count() {
        let tasks = all_uninstall_tasks();
        assert_eq!(
            tasks.len(),
            3,
            "expected 3 uninstall tasks — update all_uninstall_tasks() and this test."
        );
    }
}
