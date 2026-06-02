//! Named, dependency-ordered tasks that orchestrate resource changes.
//!
//! Tasks are filed by **domain** — what each task is about — rather than by the
//! phase in which they run.  A task's execution phase ([`TaskPhase`]) is per-task
//! metadata, independent of which domain module the task lives in, so a single
//! domain can span phases (for example `overlay` loads scripts during the
//! Repository phase and runs them during the Apply phase).
//!
//! Domain modules:
//!
//! - **Core** (`tasks::core`) — the dotfiles tool itself (binary update, wrapper
//!   installation, PATH configuration).
//! - **Repository** (`tasks::repository`) — repository synchronisation (sparse
//!   checkout, pull, config reload).
//! - **Git** (`tasks::git`) — git configuration and hooks.
//! - **Files** (`tasks::files`) — symlinks and file permissions.
//! - **Shell** (`tasks::shell`) — shell setup and completions.
//! - **System** (`tasks::system`) — OS integration (registry, PAM, systemd,
//!   developer mode, WSL).
//! - **Packages** (`tasks::packages`) — system and AUR packages.
//! - **Editors** (`tasks::editors`) — editor extensions.
//! - **AI** (`tasks::ai`) — Copilot/APM settings.
//! - **Overlay** (`tasks::overlay`) — overlay script tasks.
//! - **Validation** (`tasks::validation`) — configuration checks.
#![allow(
    clippy::arithmetic_side_effects,
    reason = "counters and validated math; bounded by config sizes"
)]

pub mod ai;
mod catalog;
pub mod core;
pub mod editors;
pub mod files;
pub(crate) mod filter;
pub mod git;
mod macros;
pub mod overlay;
pub mod packages;
pub mod repository;
pub mod shell;
pub mod system;
pub mod validation;

pub use catalog::{all_install_tasks, all_uninstall_tasks};
pub(crate) use macros::{
    process_config_resources, process_config_resources_with_provider, resource_task, task_deps,
};

// Re-export engine types so downstream `use super::` and `use crate::tasks::`
// continue to work unchanged.
pub use crate::engine::Context;
pub use crate::engine::ContextOpts;
pub(crate) use crate::engine::graph::has_cycle;
pub use crate::engine::update_signal::UpdateSignal;
#[allow(unused_imports, reason = "re-exported for doc-tests")]
// TaskStats is used by doc-tests via the lib crate
pub use crate::engine::{
    ProcessMode, ProcessOpts, ResourceAction, TaskResult, TaskStats, process_resources,
    process_resources_remove, process_resources_with_provider,
};

use std::any::TypeId;
use std::fmt;

use anyhow::Result;

use crate::logging::{DiagEvent, TaskStatus, diag_task_context};
use crate::platform::Platform;

/// Unique identifier for a task in the dependency graph.
///
/// Static task types use [`TaskId::Type`], derived from the Rust type system,
/// which is globally unique at compile time.  Dynamically created tasks — such
/// as [`OverlayScriptTask`](crate::tasks::overlay::OverlayScriptTask)
/// where multiple instances of the same struct appear in the same task list —
/// use [`TaskId::Dynamic`] with a hash computed from instance-specific data so
/// that each instance has a distinct identity.
///
/// # Examples
///
/// ```
/// use std::any::TypeId;
/// use dotfiles_cli::testing::tasks::TaskId;
///
/// // Type-based ID (the usual case):
/// let id = TaskId::Type(TypeId::of::<u32>());
///
/// // Instance-based ID (for dynamic tasks):
/// let id = TaskId::Dynamic(42);
///
/// assert_ne!(id, TaskId::Type(TypeId::of::<u32>()));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskId {
    /// Type-derived identifier for static singleton task structs.
    ///
    /// Produced automatically by the default `task_id()` implementation.
    Type(TypeId),
    /// Instance-derived identifier for dynamically created tasks.
    ///
    /// Used when multiple instances of the same struct appear in the task
    /// list (e.g. one `OverlayScriptTask` per configured script).
    Dynamic(u64),
}

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

/// Declarative rules that the orchestration layer evaluates before a task runs.
#[derive(Debug, Clone, Copy)]
pub enum ExecutionPolicy {
    /// Run whenever the task's own applicability check passes.
    Always,
    /// Run only when the current platform supports the named capability.
    PlatformSupported(&'static str, fn(&Platform) -> bool),
    /// Skip the task entirely in dry-run mode, using the given reason.
    SkipInDryRun(&'static str),
    /// The task may require elevated privileges when it predicts a mutation.
    RequiresElevation,
}

const ALWAYS_POLICY: &[ExecutionPolicy] = &[ExecutionPolicy::Always];

#[derive(Debug)]
enum PolicyDecision {
    NotApplicable(String),
    Skipped(String),
}

impl TaskPhase {
    /// Human-facing milestone label shown as a `::` header in console output.
    ///
    /// Unlike [`fmt::Display`] (which returns the bare enum variant name and is
    /// used in diagnostics and cycle-error messages), this returns an
    /// outcome-oriented phrase describing what the phase accomplishes for the
    /// user.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bootstrap => "Bootstrapping",
            Self::Repository => "Configuring repository",
            Self::Apply => "Configuring environment",
        }
    }
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

/// Subject area a task is about, independent of its execution [`TaskPhase`].
///
/// Where [`TaskPhase`] answers *when* a task runs (the scheduler groups by
/// phase to enforce ordering barriers), `Domain` answers *what* a task is
/// about.  The end-of-run summary groups by domain so the report matches the
/// user's mental model (git, packages, files…) rather than internal timing.
///
/// The two axes are genuinely independent: a single domain may span multiple
/// phases.  For example the [`Overlay`](Domain::Overlay) domain loads
/// configuration during [`TaskPhase::Repository`] and runs scripts during
/// [`TaskPhase::Apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    /// The dotfiles tool itself (binary self-update, wrapper, PATH).
    Core,
    /// The dotfiles repository (sparse checkout, pull, config reload).
    Repository,
    /// Git configuration and hooks.
    Git,
    /// System and language package installation.
    Packages,
    /// Files materialised into place (symlinks, permissions).
    Files,
    /// Shell configuration and completions.
    Shell,
    /// Operating-system integration (systemd, PAM, registry, WSL, developer mode).
    System,
    /// Editor configuration (VS Code extensions).
    Editors,
    /// AI (Copilot settings, APM packages).
    Ai,
    /// Overlay-provided configuration and custom scripts.
    Overlay,
    /// Configuration and lint validation checks.
    Validation,
    /// Default for tasks with no specific subject area (test/mock tasks only).
    General,
}

impl Domain {
    /// All domains in canonical display order, used to group summary output.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Core,
            Self::Repository,
            Self::Git,
            Self::Packages,
            Self::Files,
            Self::Shell,
            Self::System,
            Self::Editors,
            Self::Ai,
            Self::Overlay,
            Self::Validation,
            Self::General,
        ]
    }

    /// Human-facing label for this domain, shown as a summary group header.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Repository => "Repository",
            Self::Git => "Git",
            Self::Packages => "Packages",
            Self::Files => "Files",
            Self::Shell => "Shell",
            Self::System => "System",
            Self::Editors => "Editors",
            Self::Ai => "AI",
            Self::Overlay => "Overlay",
            Self::Validation => "Validation",
            Self::General => "General",
        }
    }
}

/// A named, executable task.
///
/// The `'static` bound is required so that each task struct has a stable
/// [`TaskId`] which the scheduler uses to match dependency declarations
/// (see [`Task::task_id`] and [`Task::dependencies`]).
pub trait Task: Send + Sync + 'static {
    /// Human-readable task name.
    fn name(&self) -> &str;

    /// Execution phase: [`TaskPhase::Bootstrap`], [`TaskPhase::Repository`], or [`TaskPhase::Apply`].
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

    /// Compatibility alias for callers/tests that still use sudo terminology.
    fn needs_sudo(&self, ctx: &Context) -> bool {
        self.requires_elevation(ctx)
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
fn record(
    ctx: &Context,
    name: &str,
    phase: TaskPhase,
    domain: Domain,
    status: TaskStatus,
    msg: Option<&str>,
) {
    ctx.log.record_task(name, phase, domain, status, msg);
    if !ctx.log.is_verbose() {
        ctx.log.emit_task_result(name, status, msg);
    }
}

fn evaluate_policy_decision(policies: &[ExecutionPolicy], ctx: &Context) -> Option<PolicyDecision> {
    for policy in policies {
        match *policy {
            ExecutionPolicy::Always | ExecutionPolicy::RequiresElevation => {}
            ExecutionPolicy::PlatformSupported(capability, is_supported) => {
                if !is_supported(&ctx.platform) {
                    return Some(PolicyDecision::NotApplicable(format!(
                        "{capability} not supported on {}",
                        ctx.platform
                    )));
                }
            }
            ExecutionPolicy::SkipInDryRun(reason) => {
                if ctx.dry_run {
                    return Some(PolicyDecision::Skipped(reason.to_string()));
                }
            }
        }
    }
    None
}

fn evaluate_policy(task: &dyn Task, ctx: &Context) -> Option<PolicyDecision> {
    evaluate_policy_decision(task.execution_policies(), ctx)
}

fn record_policy_decision(
    ctx: &Context,
    name: &str,
    phase: TaskPhase,
    domain: Domain,
    decision: PolicyDecision,
) {
    match decision {
        PolicyDecision::NotApplicable(reason) => {
            ctx.log.diag_task(DiagEvent::TaskSkip, name, &reason);
            if ctx.log.debug_enabled() {
                ctx.log.stage(name);
            }
            ctx.debug_fmt(|| format!("not applicable: {reason}"));
            ctx.log
                .record_task(name, phase, domain, TaskStatus::NotApplicable, None);
        }
        PolicyDecision::Skipped(reason) => {
            ctx.log.diag_task(DiagEvent::TaskSkip, name, &reason);
            ctx.log.info(&format!("skipped: {reason}"));
            record(ctx, name, phase, domain, TaskStatus::Skipped, Some(&reason));
        }
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
    let _diag_context = diag_task_context(task.name());
    let phase = task.phase();
    let domain = task.domain();

    if let Some(decision) = evaluate_policy(task, ctx) {
        record_policy_decision(ctx, task.name(), phase, domain, decision);
        return;
    }

    if !task.should_run(ctx) {
        ctx.log
            .diag_task(DiagEvent::TaskSkip, task.name(), "not applicable");
        if ctx.log.debug_enabled() {
            ctx.log.stage(task.name());
        }
        ctx.log
            .debug(&format!("skipping task: {} (not applicable)", task.name()));
        ctx.log
            .record_task(task.name(), phase, domain, TaskStatus::NotApplicable, None);
        return;
    }

    ctx.log
        .diag_task(DiagEvent::TaskStart, task.name(), "executing");
    record_run_outcome(task, ctx, phase, domain);
}

/// Run a task and record its outcome.
///
/// Cancellation-induced failures (Ctrl-C) are downgraded to
/// [`TaskStatus::Skipped`] so the summary does not count signal
/// interruptions as real failures.
fn record_run_outcome(task: &dyn Task, ctx: &Context, phase: TaskPhase, domain: Domain) {
    let rec = |status: TaskStatus, msg: Option<&str>| {
        record(ctx, task.name(), phase, domain, status, msg);
    };
    match task.run_if_applicable(ctx) {
        Ok(None) => {
            ctx.log
                .diag_task(DiagEvent::TaskSkip, task.name(), "nothing configured");
            if ctx.log.debug_enabled() {
                ctx.log.stage(task.name());
            }
            ctx.log.debug("nothing configured");
            ctx.log
                .record_task(task.name(), phase, domain, TaskStatus::NotApplicable, None);
        }
        Ok(Some(result)) => match result {
            TaskResult::Ok => {
                ctx.log.diag_task(DiagEvent::TaskDone, task.name(), "");
                rec(TaskStatus::Ok, None);
            }
            TaskResult::NotApplicable(reason) => {
                ctx.log.diag_task(DiagEvent::TaskSkip, task.name(), &reason);
                ctx.debug_fmt(|| format!("not applicable: {reason}"));
                ctx.log
                    .record_task(task.name(), phase, domain, TaskStatus::NotApplicable, None);
            }
            TaskResult::Skipped(reason) => {
                ctx.log.diag_task(DiagEvent::TaskSkip, task.name(), &reason);
                ctx.log.info(&format!("skipped: {reason}"));
                rec(TaskStatus::Skipped, Some(&reason));
            }
            TaskResult::Failed(reason) => {
                if ctx.is_cancelled() {
                    ctx.log
                        .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                    ctx.log.warn(&format!("interrupted: {reason}"));
                    rec(TaskStatus::Skipped, Some("interrupted"));
                } else {
                    ctx.log.diag_task(DiagEvent::TaskFail, task.name(), &reason);
                    ctx.log.warn(&format!("failed: {reason}"));
                    rec(TaskStatus::Failed, Some(&reason));
                }
            }
            TaskResult::DryRun => {
                ctx.log
                    .diag_task(DiagEvent::TaskDone, task.name(), "dry-run");
                rec(TaskStatus::DryRun, None);
            }
        },
        Err(e) => {
            if ctx.is_cancelled() {
                ctx.log
                    .diag_task(DiagEvent::TaskSkip, task.name(), "interrupted");
                ctx.log.warn(&format!("interrupted: {}", task.name()));
                rec(TaskStatus::Skipped, Some("interrupted"));
            } else {
                ctx.log
                    .diag_task(DiagEvent::TaskFail, task.name(), &format!("{e:#}"));
                ctx.log.error(&format!("{}: {e:#}", task.name()));
                rec(TaskStatus::Failed, Some(&format!("{e:#}")));
            }
        }
    }
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
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::resources::{IntrinsicState, Resource, ResourceChange, ResourceState};
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

    impl Resource for DummyResource {
        fn description(&self) -> String {
            "dummy".to_string()
        }

        fn apply(&self) -> Result<ResourceChange> {
            Ok(ResourceChange::AlreadyCorrect)
        }
    }

    impl IntrinsicState for DummyResource {
        fn current_state(&self) -> Result<ResourceState> {
            Ok(ResourceState::Correct)
        }
    }

    resource_task! {
        /// Test-only task for resource-task macro behaviour.
        CountingResourceTask {
            name: "Counting resource task",
            phase: TaskPhase::Apply,
            domain: Domain::General,
            items: |_ctx| {
                RESOURCE_TASK_ITEM_EVALS.with(|count| count.set(count.get() + 1));
                Vec::<()>::new()
            },
            build: |_item, _ctx| DummyResource,
            opts: ProcessOpts::strict("count"),
        }
    }

    resource_task! {
        /// Test-only task for batch-resource-task macro behaviour.
        CountingBatchTask {
            name: "Counting batch task",
            phase: TaskPhase::Apply,
            domain: Domain::General,
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

    struct PolicyTask {
        policies: &'static [ExecutionPolicy],
        ran: std::sync::Arc<std::sync::atomic::AtomicBool>,
        should_run: bool,
        needs_elevation: bool,
    }

    impl Task for PolicyTask {
        fn name(&self) -> &'static str {
            "policy-task"
        }
        fn phase(&self) -> TaskPhase {
            TaskPhase::Apply
        }
        fn execution_policies(&self) -> &[ExecutionPolicy] {
            self.policies
        }
        fn should_run(&self, _ctx: &Context) -> bool {
            self.should_run
        }
        fn needs_elevation(&self, _ctx: &Context) -> bool {
            self.needs_elevation
        }
        fn run(&self, _ctx: &Context) -> Result<TaskResult> {
            self.ran.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(TaskResult::Ok)
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
    fn execute_applies_platform_policy_before_running_task() {
        const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::PlatformSupported(
            "Windows",
            Platform::is_windows,
        )];
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, log) = make_static_context(config);
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = PolicyTask {
            policies: POLICIES,
            ran: std::sync::Arc::clone(&ran),
            should_run: true,
            needs_elevation: false,
        };

        execute(&task, &ctx);

        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(log.failure_count(), 0);
    }

    #[test]
    fn requires_elevation_respects_policy_and_dry_run() {
        const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::RequiresElevation];
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = PolicyTask {
            policies: POLICIES,
            ran,
            should_run: true,
            needs_elevation: true,
        };

        assert!(task.requires_elevation(&ctx));
        assert!(!task.requires_elevation(&ctx.with_dry_run(true)));
    }

    #[test]
    fn requires_elevation_respects_platform_policy() {
        const POLICIES: &[ExecutionPolicy] = &[
            ExecutionPolicy::PlatformSupported("Windows", Platform::is_windows),
            ExecutionPolicy::RequiresElevation,
        ];
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = PolicyTask {
            policies: POLICIES,
            ran,
            should_run: true,
            needs_elevation: true,
        };

        assert!(!task.requires_elevation(&ctx));
    }

    #[test]
    fn requires_elevation_respects_should_run() {
        const POLICIES: &[ExecutionPolicy] = &[ExecutionPolicy::RequiresElevation];
        let config = empty_config(PathBuf::from("/tmp"));
        let (ctx, _) = make_static_context(config);
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = PolicyTask {
            policies: POLICIES,
            ran,
            should_run: false,
            needs_elevation: true,
        };

        assert!(!task.requires_elevation(&ctx));
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
            23,
            "expected 23 install tasks — did you add a new task without updating \
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
