//! Named, dependency-ordered tasks that orchestrate resource changes.
//!
//! Tasks are grouped by domain (`core`, `repository`, `git`, `files`, `shell`,
//! `system`, `packages`, `editors`, `ai`, `overlay`, and `validation`) while
//! each task's [`TaskPhase`] metadata controls when it runs.  Top-level support
//! modules provide the catalog, executor, filtering, task macros, and shared
//! vocabulary.

// Task-domain modules: each groups the task definitions for one subject area.
pub mod ai;
pub mod core;
pub mod editors;
pub mod files;
pub mod git;
pub mod overlay;
pub mod packages;
pub mod repository;
pub mod shell;
pub mod system;
pub mod validation;

// Supporting infrastructure: shared machinery, not task definitions.
mod catalog;
mod execute;
pub(crate) mod filter;
mod macros;
mod types;

pub use catalog::{all_install_tasks, all_uninstall_tasks};
pub use execute::execute;
pub(crate) use macros::{
    execution_policies_impl, process_config_resources, process_config_resources_with_provider,
    resource_task, task_deps, task_metadata,
};
pub use types::{Domain, ExecutionPolicy, PlatformCapability, TaskId, TaskPhase};

// Re-export engine types so downstream `use super::` and `use crate::tasks::`
// continue to work unchanged.
pub use crate::engine::Context;
#[cfg(any(feature = "internal-api", doctest))]
pub use crate::engine::ContextOpts;
pub use crate::engine::update_signal::UpdateSignal;
pub(crate) use crate::engine::{Operation, OperationState, process_operation};
#[allow(unused_imports, reason = "re-exported for doc-tests")]
// TaskStats is used by doc-tests via the lib crate
pub use crate::engine::{
    ProcessMode, ProcessOpts, ResourceAction, TaskResult, TaskStats, process_resources,
    process_resources_remove, process_resources_with_provider,
};

use std::any::TypeId;

use anyhow::Result;

use execute::evaluate_policy_decision;

const ALWAYS_POLICY: &[ExecutionPolicy] = &[ExecutionPolicy::Always];

/// Process resources whose current state is derived from a borrowed cache.
///
/// # Errors
///
/// Returns an error if provider-backed resource processing fails.
pub(crate) fn process_resources_with_borrowed_cache<R, Cache, State>(
    ctx: &Context,
    resources: impl IntoIterator<Item = R>,
    cache: &Cache,
    state: State,
    opts: &ProcessOpts,
) -> Result<TaskResult>
where
    R: crate::resources::Resource + Send,
    Cache: Sync + ?Sized,
    State: for<'a> Fn(&'a R, &Cache) -> Result<crate::resources::ResourceState> + Sync,
{
    let provider = crate::resources::BorrowedStateProvider::new(cache, state);
    process_resources_with_provider(ctx, resources, &provider, opts)
}

/// A named, executable task.
///
/// The `'static` bound is required so that each task struct has a stable
/// [`TaskId`] which the scheduler uses to match dependency declarations
/// (see [`Task::task_id`] and [`Task::dependencies`]).
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Execution phase: [`TaskPhase::Bootstrap`], [`TaskPhase::Sync`],
    /// [`TaskPhase::Provision`], [`TaskPhase::Validation`], or [`TaskPhase::Update`].
    fn phase(&self) -> TaskPhase;

    /// Subject area this task belongs to, used to group summary output.
    ///
    /// Independent of [`phase`](Task::phase): the phase controls *when* the
    /// task runs, the domain describes *what* it is about.  The default is
    /// [`Domain::General`], reserved for test and mock tasks; every production
    /// task declares an explicit domain (enforced by a registry guard test).
    fn domain(&self) -> Domain {
        Domain::General
    }

    /// The unique identifier of this task, used by the scheduler to build the
    /// dependency graph.
    ///
    /// The default implementation returns `TaskId::Type(TypeId::of::<Self>())`
    /// which is correct for all concrete singleton task structs.  Dynamic tasks
    /// (multiple instances of the same struct in a single task list) must
    /// override this method to return `TaskId::Dynamic(hash)` with an
    /// instance-specific hash so each instance has a distinct identity.
    fn task_id(&self) -> TaskId {
        TaskId::Type(TypeId::of::<Self>())
    }

    /// Tasks that must complete before this task starts.
    ///
    /// Returns [`TaskId`]s of the concrete task structs this task depends on.
    /// The scheduler uses this information to build a dependency graph and
    /// execute independent tasks in parallel.  The default implementation
    /// returns an empty slice (no dependencies).
    ///
    /// Use the [`task_deps!`] macro to implement this method — it eliminates
    /// the manual `const DEPS` boilerplate and automatically wraps each type
    /// in [`TaskId::Type`].
    fn dependencies(&self) -> &[TaskId] {
        &[]
    }

    /// Execution rules that are enforced centrally before the task runs.
    fn execution_policies(&self) -> &[ExecutionPolicy] {
        ALWAYS_POLICY
    }

    /// Whether this task should run on the current platform/profile.
    ///
    /// Most tasks are always eligible once their execution policies pass; tasks
    /// with platform, tool-availability, or configuration gates override this.
    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

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

    /// Whether this task will need elevated privileges based on current state.
    ///
    /// Called before parallel dispatch to allow the runner to prime the
    /// credential cache (`sudo -v`) so that interactive prompts do not
    /// collide with parallel output.  The default returns `false`.
    fn needs_elevation(&self, _ctx: &Context) -> bool {
        false
    }

    /// Whether the task's policies and current state require elevation.
    fn requires_elevation(&self, ctx: &Context) -> bool {
        let declares_elevation = self
            .execution_policies()
            .iter()
            .any(|p| matches!(p, ExecutionPolicy::RequiresElevation));
        !ctx.dry_run
            && declares_elevation
            && evaluate_policy_decision(self.execution_policies(), ctx).is_none()
            && self.should_run(ctx)
            && self.needs_elevation(ctx)
    }

    /// Execute the task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute, such as when system commands
    /// fail, file operations are not permitted, or configuration is invalid.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}

/// Shared helpers for task unit tests.
///
/// Provides common mock types and factory functions so each task test module
/// does not have to duplicate boilerplate.
#[cfg(test)]
#[allow(clippy::panic, reason = "test code uses panicking helpers")]
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
    #[allow(
        clippy::expect_used,
        reason = "panicking allowed at this trust boundary"
    )]
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
            git_settings: vec![],
            copilot_settings: vec![],
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
                advance_versions: false,
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
    #[allow(clippy::struct_excessive_bools, reason = "test fixture")]
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
#[path = "tests.rs"]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests;
