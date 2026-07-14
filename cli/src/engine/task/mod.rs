//! Generic task contract: the [`Task`] trait, execution-policy vocabulary,
//! task metadata types, task macros, and the central task executor.
//!
//! Concrete task implementations live in the domain layer; this module only
//! defines the reusable machinery they build on.

mod execute;
pub(crate) mod macros;
mod types;

pub use execute::execute;
pub(crate) use macros::{
    config_resource_task, execution_policies_impl, process_config_resources,
    process_config_resources_with_provider, resource_task, task_deps, task_metadata,
};
pub use types::{Domain, ExecutionPolicy, PlatformCapability, TaskId, TaskPhase};

use std::any::TypeId;

use anyhow::Result;

use execute::evaluate_policy_decision;

use super::resource::{BorrowedStateProvider, Resource, ResourceState};
use super::{Context, ProcessOpts, TaskResult, process_resources_with_provider};

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
    R: Resource + Send,
    Cache: Sync + ?Sized,
    State: for<'a> Fn(&'a R, &Cache) -> Result<ResourceState> + Sync,
{
    let provider = BorrowedStateProvider::new(cache, state);
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
        ctx.log().stage(self.name());
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
        !ctx.dry_run()
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

/// A [`Task`] decorator that appends extra dependency [`TaskId`]s to an inner
/// task without changing any other behaviour.
///
/// The generic task machinery lets a task declare *same-layer* dependencies via
/// [`Task::dependencies`].  Cross-layer wiring — where one domain's task must
/// run after another domain's task — is deliberately kept out of the domains
/// and applied by the application layer, which is the only layer allowed to
/// name tasks across domains.  Wrapping a task in `TaskWithExtraDeps` forwards
/// its identity and behaviour unchanged while merging additional dependency
/// edges declared by the application's catalog.
///
/// Because [`task_id`](Task::task_id) is forwarded to the inner task, other
/// tasks that depend on the wrapped task by type continue to resolve correctly.
pub struct TaskWithExtraDeps {
    inner: Box<dyn Task>,
    deps: Vec<TaskId>,
}

impl TaskWithExtraDeps {
    /// Wrap `inner`, merging `extra` dependency ids with the inner task's own.
    #[must_use]
    pub fn new(inner: Box<dyn Task>, extra: &[TaskId]) -> Self {
        let mut deps: Vec<TaskId> = inner.dependencies().to_vec();
        for id in extra {
            if !deps.contains(id) {
                deps.push(*id);
            }
        }
        Self { inner, deps }
    }

    /// Wrap `inner` and box the decorator as a `dyn Task`.
    #[must_use]
    pub fn boxed(inner: Box<dyn Task>, extra: &[TaskId]) -> Box<dyn Task> {
        Box::new(Self::new(inner, extra))
    }
}

impl std::fmt::Debug for TaskWithExtraDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskWithExtraDeps")
            .field("name", &self.inner.name())
            .field("deps", &self.deps)
            .finish()
    }
}

impl Task for TaskWithExtraDeps {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn phase(&self) -> TaskPhase {
        self.inner.phase()
    }

    fn domain(&self) -> Domain {
        self.inner.domain()
    }

    fn task_id(&self) -> TaskId {
        self.inner.task_id()
    }

    fn dependencies(&self) -> &[TaskId] {
        &self.deps
    }

    fn execution_policies(&self) -> &[ExecutionPolicy] {
        self.inner.execution_policies()
    }

    fn should_run(&self, ctx: &Context) -> bool {
        self.inner.should_run(ctx)
    }

    fn run_if_applicable(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.inner.run_if_applicable(ctx)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        self.inner.needs_elevation(ctx)
    }

    fn requires_elevation(&self, ctx: &Context) -> bool {
        self.inner.requires_elevation(ctx)
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        self.inner.run(ctx)
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
