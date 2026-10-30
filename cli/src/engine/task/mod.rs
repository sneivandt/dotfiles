//! Generic task contract, metadata types, task macros, and central executor.
//!
//! Concrete task implementations live in the domain layer; this module only
//! defines the reusable machinery they build on.

mod execute;
pub(crate) mod macros;
mod types;

pub use crate::infra::logging::{TaskResultDisplay, TaskVisibility};
pub use execute::execute;
pub(crate) use execute::task_entry;
pub(crate) use execute::{TaskExecution, TaskOutcome, execute_assessed};
pub(crate) use macros::{
    config_task, run_batch_resource_task, run_resource_task, task_deps, task_metadata,
};
pub use types::{TaskAssessment, TaskId, TaskMeta};

use std::any::TypeId;

use anyhow::Result;

use super::{AccessClaim, Context, TaskResult};

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

    /// How the completed task result is presented on the console.
    ///
    /// Derived from [`Task::meta`]; do not override it.
    fn result_display(&self) -> TaskResultDisplay {
        self.meta().result_display
    }

    /// Whether this task is included only by `update`.
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

    /// Shared state held exclusively while the body executes, after dependency
    /// waits. Claims do not imply ordering or dependency failure propagation.
    /// Nested resources must claim only keys declared here when this is nonempty.
    fn access_claims(&self, _ctx: &Context) -> Vec<AccessClaim> {
        Vec::new()
    }

    /// Persistent identity across rebuilds. Display labels do not affect it.
    /// Dynamic tasks retain their explicit instance key; wrappers forward it.
    fn log_key(&self) -> String {
        let kind = std::any::type_name::<Self>();
        match self.task_id() {
            TaskId::Type(_) => kind.into(),
            TaskId::NamedDynamic { key, .. } => format!("{kind}#named:{key}"),
        }
    }

    /// Whether this task should run on the current platform/profile.
    ///
    /// Tasks with platform, tool-availability, or configuration gates override
    /// this method.
    fn should_run(&self, _ctx: &Context) -> bool {
        true
    }

    /// Whether this task will need elevated privileges based on current state.
    ///
    /// Called before parallel dispatch to allow the runner to prime the
    /// credential cache (`sudo -v`) so that interactive prompts do not
    /// collide with parallel output.  The default returns `false`.
    fn needs_elevation(&self, _ctx: &Context) -> bool {
        false
    }

    /// Read-only readiness for application-selected conditional ordering edges.
    /// False or uncertain readiness retains the declared waits. Runtime checks
    /// remain authoritative after provider tasks have finished.
    fn prerequisites_available(&self, _ctx: &Context) -> bool {
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

    /// Execute the task after dependencies finish.
    ///
    /// Check prerequisite-produced state here and return
    /// [`TaskResult::NotApplicable`] when there is no configured work. The
    /// executor owns stage announcements and outcome recording.
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

/// A [`Task`] decorator that appends application-owned dependency [`TaskId`]s
/// to an inner task without changing any other behaviour.
///
/// The generic task machinery lets a task declare *same-layer* dependencies via
/// [`Task::dependencies`].  Cross-layer wiring — where one domain's task must
/// run after another domain's task — is deliberately kept out of the domains
/// and applied by the application layer, which is the only layer allowed to name
/// tasks across domains. This decorator carries both failure-blocking and
/// ordering-only edges so a task needs at most one application wrapper.
///
/// Because [`task_id`](Task::task_id) is forwarded to the inner task, other
/// tasks that depend on the wrapped task by type continue to resolve correctly.
pub struct TaskWithExtraDeps {
    inner: Box<dyn Task>,
    deps: Vec<TaskId>,
    ordering_deps: Vec<TaskId>,
    conditional_ordering: Option<ConditionalOrdering>,
}

type ReadinessProbe = dyn Fn(&Context) -> bool + Send + Sync;

struct ConditionalOrdering {
    ids: Vec<TaskId>,
    providers_ready: Box<ReadinessProbe>,
}

impl TaskWithExtraDeps {
    /// Wrap `inner`, merging application-owned dependency ids with its own.
    #[must_use]
    pub fn new(inner: Box<dyn Task>, extra: &[TaskId], extra_ordering: &[TaskId]) -> Self {
        let merge = |existing: &[TaskId], additional: &[TaskId]| {
            let mut merged = Vec::new();
            for id in existing.iter().chain(additional) {
                if !merged.contains(id) {
                    merged.push(id.clone());
                }
            }
            merged
        };
        Self {
            deps: merge(inner.dependencies(), extra),
            ordering_deps: merge(inner.ordering_dependencies(), extra_ordering),
            inner,
            conditional_ordering: None,
        }
    }

    /// Waive selected ordering edges only when providers and consumer are ready.
    #[must_use]
    pub fn with_conditional_ordering(
        mut self,
        ids: &[TaskId],
        providers_ready: impl Fn(&Context) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.conditional_ordering = Some(ConditionalOrdering {
            ids: ids.to_vec(),
            providers_ready: Box::new(providers_ready),
        });
        self
    }

    /// Wrap `inner` and box the decorator as a `dyn Task`.
    #[must_use]
    pub fn boxed(
        inner: Box<dyn Task>,
        extra: &[TaskId],
        extra_ordering: &[TaskId],
    ) -> Box<dyn Task> {
        Box::new(Self::new(inner, extra, extra_ordering))
    }
}

impl std::fmt::Debug for TaskWithExtraDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskWithExtraDeps")
            .field("name", &self.inner.name())
            .field("deps", &self.deps)
            .field("ordering_deps", &self.ordering_deps)
            .field(
                "conditional_ordering",
                &self
                    .conditional_ordering
                    .as_ref()
                    .map(|condition| &condition.ids),
            )
            .finish()
    }
}

impl Task for TaskWithExtraDeps {
    fn log_key(&self) -> String {
        self.inner.log_key()
    }

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
        &self.ordering_deps
    }

    fn access_claims(&self, ctx: &Context) -> Vec<AccessClaim> {
        self.inner.access_claims(ctx)
    }

    fn should_run(&self, ctx: &Context) -> bool {
        self.inner.should_run(ctx)
    }

    fn needs_elevation(&self, ctx: &Context) -> bool {
        self.inner.needs_elevation(ctx)
    }

    fn prerequisites_available(&self, ctx: &Context) -> bool {
        self.inner.prerequisites_available(ctx)
    }

    fn assess(&self, ctx: &Context) -> TaskAssessment {
        let assessment = self.inner.assess(ctx);
        if assessment.is_applicable()
            && let Some(conditional) = &self.conditional_ordering
            && self.inner.prerequisites_available(ctx)
            && (conditional.providers_ready)(ctx)
        {
            return assessment.with_satisfied_ordering(&conditional.ids);
        }
        assessment
    }

    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        self.inner.run(ctx)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
