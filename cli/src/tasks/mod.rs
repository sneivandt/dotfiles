//! Named, dependency-ordered tasks that orchestrate resource changes.
pub mod chmod;
pub mod copilot_skills;
pub mod developer_mode;
pub mod git_config;
pub mod hooks;
pub mod packages;
mod processing;
pub mod registry;
pub mod reload_config;
pub mod shell;
pub mod sparse_checkout;
pub mod symlinks;
pub mod systemd_units;
pub mod update;
pub mod vscode_extensions;

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

// Re-export public items so downstream `use super::` and `use crate::tasks::`
// continue to work unchanged.
pub use processing::Context;
#[allow(unused_imports)] // TaskStats is used by doc-tests via the lib crate
pub use processing::{
    ProcessOpts, TaskResult, TaskStats, process_resource_states, process_resources,
    process_resources_remove,
};

use std::any::TypeId;

use anyhow::Result;

use crate::logging::TaskStatus;

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
    let repo_updated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    vec![
        Box::new(developer_mode::EnableDeveloperMode),
        Box::new(sparse_checkout::ConfigureSparseCheckout),
        Box::new(update::UpdateRepository::new(std::sync::Arc::clone(
            &repo_updated,
        ))),
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
        Box::new(reload_config::ReloadConfig::new(repo_updated)),
    ]
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
#[allow(clippy::panic)]
pub mod test_helpers {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::config::Config;
    use crate::config::category_matcher::Category;
    use crate::config::manifest::Manifest;
    use crate::config::profiles::Profile;
    use crate::exec::{ExecResult, Executor};
    use crate::logging::Logger;
    use crate::platform::Platform;

    use super::Context;

    /// Stub executor that panics if any real command is issued.
    ///
    /// `which()` returns the configured `which_result` value (default: `false`),
    /// which causes tasks that guard on tool availability to report
    /// *not applicable* unless explicitly overridden.
    #[derive(Debug, Default)]
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
        platform: Arc<Platform>,
        executor: Arc<dyn Executor>,
    ) -> Context {
        Context {
            config: std::sync::Arc::new(std::sync::RwLock::new(config)),
            platform,
            log: Arc::new(Logger::new("test")),
            dry_run: false,
            home: PathBuf::from("/home/test"),
            executor,
            parallel: false,
        }
    }

    /// Build a [`Context`] with the specified OS/arch and default [`WhichExecutor`].
    ///
    /// Replaces the per-platform helpers (`make_linux_context`, etc.) with a
    /// single generic factory, reducing near-identical code.
    #[must_use]
    pub fn make_platform_context(
        config: Config,
        os: crate::platform::Os,
        is_arch: bool,
    ) -> Context {
        make_platform_context_with_which(config, os, is_arch, false)
    }

    /// Build a [`Context`] with the specified OS/arch and a [`WhichExecutor`]
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
        make_context(
            config,
            Arc::new(Platform::new(os, is_arch)),
            Arc::new(WhichExecutor { which_result }),
        )
    }

    /// Build a [`Context`] with a Linux non-arch platform and default [`WhichExecutor`].
    ///
    /// Convenience shorthand for tests that only need a plain Linux context.
    #[must_use]
    pub fn make_linux_context(config: Config) -> Context {
        make_platform_context(config, crate::platform::Os::Linux, false)
    }

    /// Build a [`Context`] with a Windows platform and default [`WhichExecutor`].
    ///
    /// Convenience shorthand for tests that only need a plain Windows context.
    #[must_use]
    pub fn make_windows_context(config: Config) -> Context {
        make_platform_context(config, crate::platform::Os::Windows, false)
    }

    /// Build a [`Context`] with an Arch Linux platform and default [`WhichExecutor`].
    ///
    /// Convenience shorthand for tests that target Arch-specific behaviour.
    #[must_use]
    pub fn make_arch_context(config: Config) -> Context {
        make_platform_context(config, crate::platform::Os::Linux, true)
    }

    /// Build a [`Context`] with a default Linux platform and
    /// default [`WhichExecutor`], also returning the [`Logger`] so tests can
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
    use std::path::PathBuf;
    use test_helpers::{empty_config, make_static_context};

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
}
