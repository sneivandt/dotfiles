//! Named, dependency-ordered tasks that orchestrate resource changes.
pub mod chmod;
pub mod copilot_skills;
pub mod developer_mode;
pub mod git_config;
pub mod hooks;
pub mod packages;
pub mod registry;
pub mod reload_config;
pub mod self_update;
pub mod shell;
pub mod sparse_checkout;
pub mod symlinks;
pub mod systemd_units;
pub mod update;
pub mod validation;
pub mod vscode_extensions;
pub mod wsl_conf;

/// Implement [`Task::dependencies`] by expanding to the required
/// `fn dependencies(&self) -> &[TypeId]` method body.
///
/// The `const DEPS` intermediate is required because [`std::any::TypeId::of`]
/// is a `const fn` — placing it in a `const` ensures the slice has a
/// `'static` lifetime as required by the return type.
///
/// # Examples
///
/// ```ignore
/// task_deps![super::reload_config::ReloadConfig, super::symlinks::InstallSymlinks]
/// // expands to:
/// //   fn dependencies(&self) -> &[std::any::TypeId] {
/// //       const DEPS: &[std::any::TypeId] = &[
/// //           std::any::TypeId::of::<super::reload_config::ReloadConfig>(),
/// //           std::any::TypeId::of::<super::symlinks::InstallSymlinks>(),
/// //       ];
/// //       DEPS
/// //   }
/// ```
macro_rules! task_deps {
    [$($dep:ty),+ $(,)?] => {
        fn dependencies(&self) -> &[std::any::TypeId] {
            const DEPS: &[std::any::TypeId] = &[$(std::any::TypeId::of::<$dep>()),+];
            DEPS
        }
    };
}

pub(crate) use task_deps;

// Re-export engine types so downstream `use super::` and `use crate::tasks::`
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

use anyhow::Result;

use crate::logging::TaskStatus;
use crate::resources::ResourceState;

/// Execute the standard resource-task pattern: collect config items, build
/// resources, and delegate to [`process_resources`].
///
/// Returns `Ok(TaskResult::NotApplicable("nothing configured"))` when `items`
/// is empty.
///
/// # Errors
///
/// Returns an error if any resource fails to apply and the `opts` mode
/// requires bailing on errors.
pub fn run_resource_task<I, R>(
    ctx: &Context,
    items: Vec<I>,
    mut build: impl FnMut(I, &Context) -> R,
    opts: &ProcessOpts,
) -> Result<TaskResult>
where
    R: crate::resources::Resource + Send,
{
    if items.is_empty() {
        return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
    }
    process_resources(ctx, items.into_iter().map(|item| build(item, ctx)), opts)
}

/// Execute the standard batch-resource-task pattern: collect config items,
/// run a single bulk-state query, build `(Resource, ResourceState)` pairs,
/// and delegate to [`process_resource_states`].
///
/// Returns `Ok(TaskResult::NotApplicable("nothing configured"))` when `items`
/// is empty.
///
/// # Errors
///
/// Returns an error if the bulk state query fails, or if any resource fails
/// to apply and the `opts` mode requires bailing.
pub fn run_batch_resource_task<I, R, C>(
    ctx: &Context,
    items: Vec<I>,
    query_cache: impl FnOnce(&[I], &Context) -> Result<C>,
    mut build: impl FnMut(I, &Context) -> R,
    state_for: impl Fn(&R, &C) -> ResourceState,
    opts: &ProcessOpts,
) -> Result<TaskResult>
where
    R: crate::resources::Applicable + Send,
{
    if items.is_empty() {
        return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
    }
    ctx.log.debug(&format!(
        "batch-checking {} resources with a single query",
        items.len()
    ));
    let cache = query_cache(&items, ctx)?;
    let resource_states = items.into_iter().map(|item| {
        let resource = build(item, ctx);
        let state = state_for(&resource, &cache);
        (resource, state)
    });
    process_resource_states(ctx, resource_states, opts)
}

/// A named, executable task.
///
/// The `'static` bound is required so that each task struct has a stable
/// [`TypeId`] which the scheduler uses to match dependency declarations
/// (see [`Task::task_id`] and [`Task::dependencies`]).
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

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
    /// implementation simply delegates to [`Task::run`]; tasks can override it
    /// to avoid evaluating config twice.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute.
    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.run(ctx).map(Some)
    }

    /// Execute the task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute, such as when system commands
    /// fail, file operations are not permitted, or configuration is invalid.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}

/// The complete set of tasks run by the uninstall command.
#[must_use]
pub fn all_uninstall_tasks() -> Vec<Box<dyn Task>> {
    vec![
        Box::new(symlinks::UninstallSymlinks),
        Box::new(hooks::UninstallGitHooks::new()),
    ]
}

/// The complete set of tasks run by the install command.
///
/// Order within the list is arbitrary — the scheduler derives execution order
/// from each task's [`Task::dependencies`] declaration.
#[must_use]
pub fn all_install_tasks() -> Vec<Box<dyn Task>> {
    let repo_updated = UpdateSignal::new();
    vec![
        Box::new(self_update::UpdateBinary),
        Box::new(developer_mode::EnableDeveloperMode),
        Box::new(sparse_checkout::ConfigureSparseCheckout::new()),
        Box::new(update::UpdateRepository::new(repo_updated.clone())),
        Box::new(git_config::ConfigureGit),
        Box::new(hooks::InstallGitHooks::new()),
        Box::new(packages::InstallPackages),
        Box::new(packages::InstallParu),
        Box::new(packages::InstallAurPackages),
        Box::new(symlinks::InstallSymlinks),
        Box::new(chmod::ApplyFilePermissions),
        Box::new(shell::ConfigureShell),
        Box::new(systemd_units::ConfigureSystemd),
        Box::new(registry::ApplyRegistry),
        Box::new(vscode_extensions::InstallVsCodeExtensions),
        Box::new(copilot_skills::InstallCopilotSkills),
        Box::new(wsl_conf::InstallWslConf),
        Box::new(reload_config::ReloadConfig::new(repo_updated)),
    ]
}

/// Execute a task, recording the result in the logger.
///
/// Each task invocation is wrapped in a [`tracing::info_span`] so that
/// the log file and diagnostic output include structured context about
/// which task produced each message.
pub fn execute(task: &dyn Task, ctx: &Context) {
    let span = tracing::info_span!("task", name = task.name());
    let _enter = span.enter();

    if !task.should_run(ctx) {
        ctx.log
            .debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log
            .record_task(task.name(), TaskStatus::NotApplicable, None);
        return;
    }

    ctx.log.stage(task.name());

    match task.run_if_applicable(ctx) {
        Ok(None) => {
            ctx.log.info("nothing configured");
            ctx.log
                .record_task(task.name(), TaskStatus::NotApplicable, None);
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log.record_task(task.name(), TaskStatus::Ok, None);
            }
            TaskResult::NotApplicable(reason) => {
                ctx.log.debug(&format!("not applicable: {reason}"));
                ctx.log
                    .record_task(task.name(), TaskStatus::NotApplicable, None);
            }
            TaskResult::Skipped(reason) => {
                ctx.log.info(&format!("skipped: {reason}"));
                ctx.log
                    .record_task(task.name(), TaskStatus::Skipped, Some(&reason));
            }
            TaskResult::Failed(reason) => {
                ctx.log.warn(&format!("failed: {reason}"));
                ctx.log
                    .record_task(task.name(), TaskStatus::Failed, Some(&reason));
            }
            TaskResult::DryRun => {
                ctx.log.record_task(task.name(), TaskStatus::DryRun, None);
            }
        },
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
#[allow(clippy::panic)]
pub mod test_helpers {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::config::Config;
    use crate::config::category_matcher::Category;
    use crate::config::manifest::Manifest;
    use crate::config::profiles::Profile;
    use crate::exec::Executor;
    use crate::logging::Logger;
    use crate::platform::Platform;

    use super::Context;

    /// Re-export [`TestExecutor`](crate::exec::test_helpers::TestExecutor) for
    /// convenience.
    pub use crate::exec::test_helpers::TestExecutor;

    /// Build a [`Config`] with all lists empty and `root` set to `root`.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn empty_config(root: PathBuf) -> Config {
        Config {
            root,
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
            copilot_skills: vec![],
            git_settings: vec![],
            manifest: Manifest {
                excluded_files: vec![],
            },
        }
    }

    /// Build a [`Context`] from the given config, platform and executor.
    pub fn make_context(
        config: Config,
        platform: Platform,
        executor: Arc<dyn Executor>,
    ) -> Context {
        Context::from_raw(
            std::sync::Arc::new(std::sync::RwLock::new(std::sync::Arc::new(config))),
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
                Arc::new(TestExecutor::stub().with_which(self.which_result)),
            )
            .with_ci(self.is_ci)
        }
    }

    /// Build a [`Context`] with the specified OS/arch and default [`TestExecutor`].
    #[must_use]
    pub fn make_platform_context(
        config: Config,
        os: crate::platform::Os,
        is_arch: bool,
    ) -> Context {
        ContextBuilder::new(config).os(os).arch(is_arch).build()
    }

    /// Build a [`Context`] with the specified OS/arch and a [`TestExecutor`]
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

    /// Build a [`Context`] with a Linux non-arch platform and default [`TestExecutor`].
    ///
    /// Convenience shorthand for tests that only need a plain Linux context.
    #[must_use]
    pub fn make_linux_context(config: Config) -> Context {
        ContextBuilder::new(config).build()
    }

    /// Build a [`Context`] with a Windows platform and default [`TestExecutor`].
    ///
    /// Convenience shorthand for tests that only need a plain Windows context.
    #[must_use]
    pub fn make_windows_context(config: Config) -> Context {
        ContextBuilder::new(config)
            .os(crate::platform::Os::Windows)
            .build()
    }

    /// Build a [`Context`] with an Arch Linux platform and default [`TestExecutor`].
    ///
    /// Convenience shorthand for tests that target Arch-specific behaviour.
    #[must_use]
    pub fn make_arch_context(config: Config) -> Context {
        ContextBuilder::new(config).arch(true).build()
    }

    /// Build a [`Context`] with a default Linux platform and
    /// default [`TestExecutor`], also returning the [`Logger`] so tests can
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

    /// Test-only task for resource-task function behaviour.
    #[derive(Debug)]
    struct CountingResourceTask;

    impl Task for CountingResourceTask {
        fn name(&self) -> &'static str {
            "Counting resource task"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
            let items: Vec<()> = {
                RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::new()
            };
            if items.is_empty() {
                return Ok(None);
            }
            run_resource_task(
                ctx,
                items,
                |_item, _ctx| DummyResource,
                &ProcessOpts::strict("count"),
            )
            .map(Some)
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            let items: Vec<()> = {
                RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::new()
            };
            if items.is_empty() {
                return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
            }
            run_resource_task(
                ctx,
                items,
                |_item, _ctx| DummyResource,
                &ProcessOpts::strict("count"),
            )
        }
    }

    /// Test-only task for batch-resource-task function behaviour.
    #[derive(Debug)]
    struct CountingBatchTask;

    impl Task for CountingBatchTask {
        fn name(&self) -> &'static str {
            "Counting batch task"
        }

        fn should_run(&self, _ctx: &Context) -> bool {
            true
        }

        fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
            let items: Vec<()> = {
                BATCH_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::new()
            };
            if items.is_empty() {
                return Ok(None);
            }
            run_batch_resource_task(
                ctx,
                items,
                |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
                |_item, _ctx| DummyResource,
                |_resource, _cache| ResourceState::Correct,
                &ProcessOpts::strict("count"),
            )
            .map(Some)
        }

        fn run(&self, ctx: &Context) -> Result<TaskResult> {
            let items: Vec<()> = {
                BATCH_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::new()
            };
            if items.is_empty() {
                return Ok(TaskResult::NotApplicable("nothing configured".to_string()));
            }
            run_batch_resource_task(
                ctx,
                items,
                |_items, _ctx| Ok::<Vec<()>, anyhow::Error>(Vec::new()),
                |_item, _ctx| DummyResource,
                |_resource, _cache| ResourceState::Correct,
                &ProcessOpts::strict("count"),
            )
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
    // run_resource_task free function
    // ------------------------------------------------------------------

    #[test]
    fn run_resource_task_returns_not_applicable_for_empty_items() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let result = run_resource_task(
            &ctx,
            Vec::<()>::new(),
            |(), _ctx| DummyResource,
            &ProcessOpts::strict("test"),
        )
        .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_resource_task_processes_items() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let items = vec![(), ()];
        let result = run_resource_task(
            &ctx,
            items,
            |(), _ctx| DummyResource,
            &ProcessOpts::strict("test"),
        )
        .unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok, got {result:?}"
        );
    }

    // ------------------------------------------------------------------
    // run_batch_resource_task free function
    // ------------------------------------------------------------------

    #[test]
    fn run_batch_resource_task_returns_not_applicable_for_empty_items() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let result = run_batch_resource_task(
            &ctx,
            Vec::<()>::new(),
            |_items, _ctx| Ok::<(), anyhow::Error>(()),
            |_item, _ctx| DummyResource,
            |_resource, _cache| ResourceState::Correct,
            &ProcessOpts::strict("test"),
        )
        .unwrap();
        assert!(matches!(result, TaskResult::NotApplicable(_)));
    }

    #[test]
    fn run_batch_resource_task_processes_items() {
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let items = vec![(), ()];
        let result = run_batch_resource_task(
            &ctx,
            items,
            |_items, _ctx| Ok::<(), anyhow::Error>(()),
            |_item, _ctx| DummyResource,
            |_resource, _cache| ResourceState::Correct,
            &ProcessOpts::strict("test"),
        )
        .unwrap();
        assert!(
            matches!(result, TaskResult::Ok),
            "expected Ok, got {result:?}"
        );
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
            18,
            "expected 18 install tasks — did you add a new task without updating \
             all_install_tasks()? Update the registration list and this test."
        );
    }

    #[test]
    fn all_uninstall_tasks_count() {
        let tasks = all_uninstall_tasks();
        assert_eq!(
            tasks.len(),
            2,
            "expected 2 uninstall tasks — update all_uninstall_tasks() and this test."
        );
    }
}
