//! Generic task contract, metadata types, task macros, and central executor.
//!
//! Concrete task implementations live in the domain layer; this module only
//! defines the reusable machinery they build on.

mod execute;
pub(crate) mod macros;
mod types;

use crate::infra::logging::OutputExt as _;
pub use crate::infra::logging::TaskVisibility;
pub use execute::execute;
pub(crate) use execute::execute_assessed;
pub(crate) use macros::{
    configured_task_result, run_batch_resource_task, run_resource_task, task_deps, task_metadata,
};
pub use types::{TaskAssessment, TaskId, TaskMeta};

use std::any::TypeId;

use anyhow::Result;

use super::{Context, TaskResult};

/// A named, executable task.
///
/// The `'static` bound is required so that each task struct has a stable
/// [`TaskId`] which the scheduler uses to match dependency declarations
/// (see [`Task::task_id`] and [`Task::dependencies`]).
pub trait Task: Send + Sync + 'static {
    /// Static descriptive metadata: name, selector, visibility, and whether the
    /// task belongs to `update` only.
    ///
    /// Use the [`task_metadata!`] macro to implement this — it turns a
    /// declarative block into the single method body.
    fn meta(&self) -> TaskMeta<'_>;

    /// Human-readable task name.
    ///
    /// Derived from [`Task::meta`]; do not override it, or the name reported to
    /// the user can disagree with the one a decorator forwards.
    fn name(&self) -> &str {
        self.meta().name
    }

    /// Stable selector used by `--only` and `--skip`.
    ///
    /// Derived from [`Task::meta`]; do not override it.
    fn selector(&self) -> &str {
        self.meta().selector()
    }

    /// Whether this task is shown in user-facing discovery and results.
    ///
    /// Derived from [`Task::meta`]; do not override it.
    fn visibility(&self) -> TaskVisibility {
        self.meta().visibility
    }

    /// Whether this task is included only by the `update` command.
    ///
    /// Derived from [`Task::meta`]; do not override it.
    fn update_only(&self) -> bool {
        self.meta().update_only
    }

    /// The unique identifier of this task, used by the scheduler to build the
    /// dependency graph.
    ///
    /// The default implementation returns `TaskId::Type(TypeId::of::<Self>())`
    /// which is correct for all concrete singleton task structs. Dynamic tasks
    /// (multiple instances of the same struct in a single task list) must
    /// override this method and use [`TaskId::dynamic`] with a stable
    /// instance-specific key.
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

    /// Tasks that must finish before this task starts, but whose failure does
    /// not make this task's own work invalid.
    ///
    /// Use [`dependencies`](Self::dependencies) for prerequisites whose
    /// failure must block this task.
    fn ordering_dependencies(&self) -> &[TaskId] {
        &[]
    }

    /// Whether this task should run on the current platform/profile.
    ///
    /// Tasks with platform, tool-availability, or configuration gates override
    /// this method.
    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    /// Execute the task when it has configured work.
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
    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        ctx.log().task_stage(self.name());
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

    /// Assess this task once for the current execution phase.
    ///
    /// The default preserves the existing task hooks while ensuring their
    /// filesystem and tool probes are evaluated only once before elevation
    /// planning and scheduling.
    fn assess(&self, ctx: &Context) -> TaskAssessment {
        if !self.should_run(ctx) {
            return TaskAssessment::not_applicable(None::<String>);
        }
        TaskAssessment::applicable().with_elevation(!ctx.dry_run() && self.needs_elevation(ctx))
    }

    /// Execute the task.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails to execute, such as when system commands
    /// fail, file operations are not permitted, or configuration is invalid.
    fn run(&self, ctx: &Context) -> Result<TaskResult>;
}

/// Whether a task's single assessment requires elevation.
#[must_use]
#[cfg(test)]
pub fn requires_elevation(task: &dyn Task, ctx: &Context) -> bool {
    let assessment = task.assess(ctx);
    assessment.is_applicable() && assessment.requires_elevation()
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
        let mut deps = Vec::new();
        for id in inner.dependencies().iter().chain(extra) {
            if !deps.contains(id) {
                deps.push(id.clone());
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
    fn meta(&self) -> TaskMeta<'_> {
        self.inner.meta()
    }

    fn task_id(&self) -> TaskId {
        self.inner.task_id()
    }

    fn dependencies(&self) -> &[TaskId] {
        &self.deps
    }

    fn ordering_dependencies(&self) -> &[TaskId] {
        self.inner.ordering_dependencies()
    }

    fn should_run(&self, ctx: &Context) -> bool {
        self.inner.should_run(ctx)
    }

    fn run_configured(&self, ctx: &Context) -> Result<Option<TaskResult>> {
        self.inner.run_configured(ctx)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        self.inner.needs_elevation(ctx)
    }

    fn assess(&self, ctx: &Context) -> TaskAssessment {
        self.inner.assess(ctx)
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
