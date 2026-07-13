//! Generic lifecycle for idempotent multi-step operations.
//!
//! Operations cover task bodies that are still checkable and idempotent, but do
//! not fit the one-resource check/apply shape.  Examples include repository
//! synchronization, sparse-checkout rewrites, generated files, and tool-driven
//! workflows.

use anyhow::Result;

use super::context::Context;
use super::stats::TaskResult;

/// Current lifecycle state and immutable execution plan for an [`Operation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperationState<Plan> {
    /// The operation's desired post-condition is already satisfied.
    Complete,
    /// The operation should run to converge state.
    NeedsRun {
        /// Human-readable reason the operation needs to run.
        reason: String,
        /// Immutable data discovered while checking current state.
        plan: Plan,
    },
    /// The operation cannot safely run right now.
    Blocked {
        /// Human-readable reason the operation is blocked.
        reason: String,
    },
    /// The operation does not apply to this environment.
    NotApplicable {
        /// Human-readable reason the operation is not applicable.
        reason: String,
    },
}

impl<Plan> OperationState<Plan> {
    /// Create a [`NeedsRun`](Self::NeedsRun) state.
    pub(crate) fn needs_run(reason: impl Into<String>, plan: Plan) -> Self {
        Self::NeedsRun {
            reason: reason.into(),
            plan,
        }
    }

    /// Create a [`Blocked`](Self::Blocked) state.
    pub(crate) fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked {
            reason: reason.into(),
        }
    }

    /// Create a [`NotApplicable`](Self::NotApplicable) state.
    pub(crate) fn not_applicable(reason: impl Into<String>) -> Self {
        Self::NotApplicable {
            reason: reason.into(),
        }
    }
}

/// Idempotent, checkable task body that does not fit the [`Resource`](crate::engine::Resource)
/// model.
pub(crate) trait Operation {
    /// Immutable data discovered during state inspection and consumed by
    /// preview or apply.
    type Plan;

    /// Inspect current state without mutating anything.
    ///
    /// Implementations that invoke opaque external scripts can only enforce
    /// this contract cooperatively. In particular, overlay scripts must honor
    /// their documented `--check` mode.
    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>>;

    /// Preview the change for dry-run mode.
    ///
    /// External scripts must honor their documented dry-run argument because
    /// the engine cannot sandbox or otherwise prevent script-side mutations.
    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult>;

    /// Apply the operation after [`current_state`](Self::current_state) reports
    /// [`OperationState::NeedsRun`].
    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult>;
}

/// Execute an [`Operation`] using the standard check → dry-run → apply order.
///
/// # Errors
///
/// Returns an error if state discovery, dry-run preview, or apply fails.
pub(crate) fn process_operation(ctx: &Context, operation: &impl Operation) -> Result<TaskResult> {
    let state = operation.current_state(ctx)?;
    match state {
        OperationState::Complete => {
            ctx.log.debug("already complete");
            Ok(TaskResult::Ok)
        }
        OperationState::NotApplicable { reason } => {
            ctx.debug_fmt(|| format!("not applicable: {reason}"));
            Ok(TaskResult::NotApplicable(reason))
        }
        OperationState::Blocked { reason } => {
            ctx.log.info(&format!("skipped: {reason}"));
            Ok(TaskResult::Skipped(reason))
        }
        OperationState::NeedsRun { plan, .. } if ctx.dry_run => operation.preview(ctx, &plan),
        OperationState::NeedsRun { plan, .. } => operation.apply(ctx, &plan),
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::test_helpers::{empty_config, make_linux_context};

    #[derive(Debug, Clone)]
    struct TestOperation {
        state: OperationState<&'static str>,
        preview_calls: Arc<AtomicUsize>,
        apply_calls: Arc<AtomicUsize>,
    }

    impl TestOperation {
        fn new(state: OperationState<&'static str>) -> Self {
            Self {
                state,
                preview_calls: Arc::new(AtomicUsize::new(0)),
                apply_calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn preview_calls(&self) -> usize {
            self.preview_calls.load(Ordering::SeqCst)
        }

        fn apply_calls(&self) -> usize {
            self.apply_calls.load(Ordering::SeqCst)
        }
    }

    impl Operation for TestOperation {
        type Plan = &'static str;

        fn current_state(&self, _ctx: &Context) -> Result<OperationState<Self::Plan>> {
            Ok(self.state.clone())
        }

        fn preview(&self, _ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
            assert_eq!(
                *plan, "planned change",
                "preview should receive checked plan"
            );
            self.preview_calls.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::DryRun)
        }

        fn apply(&self, _ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
            assert_eq!(*plan, "planned change", "apply should receive checked plan");
            self.apply_calls.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResult::Ok)
        }
    }

    fn test_context() -> Context {
        make_linux_context(empty_config(PathBuf::from("/tmp")))
    }

    #[test]
    fn complete_operation_noops() {
        let ctx = test_context();
        let operation = TestOperation::new(OperationState::Complete);

        let result = process_operation(&ctx, &operation).unwrap();

        assert!(matches!(result, TaskResult::Ok));
        assert_eq!(operation.preview_calls(), 0);
        assert_eq!(operation.apply_calls(), 0);
    }

    #[test]
    fn dry_run_previews_without_applying() {
        let ctx = test_context().with_dry_run(true);
        let operation =
            TestOperation::new(OperationState::needs_run("write file", "planned change"));

        let result = process_operation(&ctx, &operation).unwrap();

        assert!(matches!(result, TaskResult::DryRun));
        assert_eq!(operation.preview_calls(), 1);
        assert_eq!(operation.apply_calls(), 0);
    }

    #[test]
    fn needs_run_applies_outside_dry_run() {
        let ctx = test_context();
        let operation =
            TestOperation::new(OperationState::needs_run("write file", "planned change"));

        let result = process_operation(&ctx, &operation).unwrap();

        assert!(matches!(result, TaskResult::Ok));
        assert_eq!(operation.preview_calls(), 0);
        assert_eq!(operation.apply_calls(), 1);
    }

    #[test]
    fn blocked_operation_skips_without_preview_or_apply() {
        let ctx = test_context();
        let operation = TestOperation::new(OperationState::blocked("local changes present"));

        let result = process_operation(&ctx, &operation).unwrap();

        assert!(matches!(result, TaskResult::Skipped(reason) if reason == "local changes present"));
        assert_eq!(operation.preview_calls(), 0);
        assert_eq!(operation.apply_calls(), 0);
    }

    #[test]
    fn not_applicable_operation_reports_not_applicable() {
        let ctx = test_context();
        let operation = TestOperation::new(OperationState::not_applicable("tool missing"));

        let result = process_operation(&ctx, &operation).unwrap();

        assert!(matches!(result, TaskResult::NotApplicable(reason) if reason == "tool missing"));
        assert_eq!(operation.preview_calls(), 0);
        assert_eq!(operation.apply_calls(), 0);
    }
}
