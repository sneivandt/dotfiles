//! Stress tests for the parallel resource pipeline.
//!
//! The parallel path is where a regression is most likely to be silent: work
//! items are folded into per-thread [`TaskStats`] and merged in a tree
//! reduction, buffered console output is replayed per task, and cancellation is
//! checked cooperatively between items. These tests drive batches large enough
//! to exercise several Rayon threads while keeping every assertion
//! deterministic — they assert on aggregate counts, set membership, and
//! monotonic progress, never on a particular interleaving.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::engine::mode::ProcessOpts;
use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
    TaskResult, process_resources, process_resources_remove,
};
use crate::infra::cancellation::CancellationToken;
use crate::infra::logging::{MsgKind, Output, TaskRecorder, TaskStatus};
use crate::test_helpers::{FailAt, FailingResource, empty_config};

use super::{bail_opts, parallel_context};

/// Batch size used by the stress tests.
///
/// Large enough that Rayon splits the work across threads on any realistic
/// runner, small enough to stay fast.
const BATCH: usize = 64;

/// Extract the batch counters `(changed, already_ok, failed)` from a result.
fn stats(result: &TaskResult) -> (u32, u32, u32) {
    match result {
        TaskResult::Batch(stats) => Some((
            stats.changed_count(),
            stats.already_ok_count(),
            stats.failed_count(),
        )),
        TaskResult::Ok
        | TaskResult::DryRun
        | TaskResult::CheckPassed
        | TaskResult::NotApplicable(_)
        | TaskResult::Skipped { .. }
        | TaskResult::Failed(_) => None,
    }
    .expect("resource processing should produce a batch result")
}

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// Records every message emitted, from any thread.
#[derive(Debug, Default)]
struct RecordingLog {
    messages: Mutex<Vec<String>>,
}

impl RecordingLog {
    fn messages(&self) -> Vec<String> {
        self.messages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Output for RecordingLog {
    fn emit(&self, _kind: MsgKind, msg: std::borrow::Cow<'_, str>) {
        self.messages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(msg.into_owned());
    }
}

impl TaskRecorder for RecordingLog {
    fn record_task(&self, _name: &str, _status: TaskStatus, _message: Option<&str>) {}
}

/// Resource that counts concurrent `apply()` calls and records the peak.
#[derive(Debug)]
struct ConcurrencyProbe {
    id: usize,
    in_flight: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Resource for ConcurrencyProbe {
    fn description(&self) -> String {
        format!("probe {}", self.id)
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let current = self
            .in_flight
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        self.peak.fetch_max(current, Ordering::SeqCst);
        // Hold the slot briefly so overlapping work is observable without
        // making the test depend on the delay for correctness.
        std::thread::yield_now();
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for ConcurrencyProbe {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for ConcurrencyProbe {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        Ok(ResourceState::Missing)
    }
}

/// Resource that cancels the run once a threshold of applies is reached.
#[derive(Debug)]
struct CancellingResource {
    applied: Arc<AtomicUsize>,
    cancel_after: usize,
    token: CancellationToken,
}

impl Resource for CancellingResource {
    fn description(&self) -> String {
        "cancelling resource".to_string()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let count = self
            .applied
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        if count >= self.cancel_after {
            self.token.cancel();
        }
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for CancellingResource {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        self.apply()
    }
}

impl IntrinsicState for CancellingResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        Ok(ResourceState::Correct)
    }
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

#[test]
fn parallel_apply_accounts_for_every_resource_exactly_once() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let resources: Vec<ConcurrencyProbe> = (0..BATCH)
        .map(|id| ConcurrencyProbe {
            id,
            in_flight: Arc::clone(&in_flight),
            peak: Arc::clone(&peak),
        })
        .collect();

    let result = process_resources(&ctx, resources, &ProcessOpts::lenient("install"))
        .expect("parallel apply should succeed");

    let (changed, already_ok, failed) = stats(&result);
    assert_eq!(changed, u32::try_from(BATCH).expect("batch fits in u32"));
    assert_eq!(already_ok, 0);
    assert_eq!(failed, 0);
    assert_eq!(
        in_flight.load(Ordering::SeqCst),
        0,
        "every apply should have released its slot"
    );
    assert!(
        peak.load(Ordering::SeqCst) >= 1,
        "at least one apply must have been observed in flight"
    );
}

#[test]
fn parallel_apply_merges_mixed_outcomes_without_losing_counts() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));

    // A third need work, a third are already converged, a third fail.
    let resources: Vec<FailingResource> = (0..BATCH)
        .map(|idx| match idx % 3 {
            0 => FailingResource::new(format!("changed-{idx}"), FailAt::Never),
            1 => FailingResource::new(format!("ok-{idx}"), FailAt::Never)
                .with_state(ResourceState::Correct),
            _ => FailingResource::new(format!("failing-{idx}"), FailAt::Always),
        })
        .collect();

    let result = process_resources(&ctx, resources, &ProcessOpts::lenient("install"))
        .expect("lenient mode should not abort on resource failures");

    let (changed, already_ok, failed) = stats(&result);
    assert_eq!(
        changed + already_ok + failed,
        u32::try_from(BATCH).expect("batch fits in u32"),
        "every resource must be accounted for exactly once"
    );
    assert_eq!(changed, 22);
    assert_eq!(already_ok, 21);
    assert_eq!(failed, 21);
}

#[test]
fn parallel_remove_accounts_for_every_resource_exactly_once() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));

    let resources: Vec<FailingResource> = (0..BATCH)
        .map(|idx| {
            FailingResource::new(format!("removable-{idx}"), FailAt::Never)
                .with_state(ResourceState::Correct)
        })
        .collect();

    let result = process_resources_remove(&ctx, resources, "remove")
        .expect("parallel remove should succeed");

    let (changed, _already_ok, failed) = stats(&result);
    assert_eq!(changed, u32::try_from(BATCH).expect("batch fits in u32"));
    assert_eq!(failed, 0);
}

// ---------------------------------------------------------------------------
// Error propagation
// ---------------------------------------------------------------------------

#[test]
fn parallel_apply_propagates_the_first_failure_in_strict_mode() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));

    let resources: Vec<FailingResource> = (0..BATCH)
        .map(|idx| {
            let fail = if idx == BATCH / 2 {
                FailAt::Always
            } else {
                FailAt::Never
            };
            FailingResource::new(format!("resource-{idx}"), fail)
        })
        .collect();

    let err = process_resources(&ctx, resources, &bail_opts())
        .expect_err("strict mode must surface the injected failure");
    assert!(
        format!("{err:#}").contains("injected failure"),
        "unexpected error: {err:#}"
    );
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

#[test]
fn cancellation_mid_batch_stops_dispatching_new_apply_work() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));
    let token = CancellationToken::new();
    let ctx = ctx.with_cancellation(token.clone());
    let recorder = Arc::new(RecordingLog::default());
    let log: Arc<dyn crate::infra::logging::Log> = Arc::<RecordingLog>::clone(&recorder);
    let ctx = ctx.with_log(log);
    let applied = Arc::new(AtomicUsize::new(0));

    let resources: Vec<CancellingResource> = (0..BATCH)
        .map(|_| CancellingResource {
            applied: Arc::clone(&applied),
            cancel_after: 1,
            token: token.clone(),
        })
        .collect();

    let result = process_resources_remove(&ctx, resources, "remove")
        .expect("cancellation is cooperative, not an error");

    let (changed, _already_ok, failed) = stats(&result);
    assert_eq!(failed, 0, "cancellation must not be reported as a failure");
    assert!(
        changed < u32::try_from(BATCH).expect("batch fits in u32"),
        "cancellation should have skipped at least one item, saw {changed}"
    );
    assert_eq!(
        u64::from(changed),
        u64::try_from(applied.load(Ordering::SeqCst)).expect("count fits in u64"),
        "reported changes must match the work actually performed"
    );
    assert!(ctx.is_cancelled());

    // The parallel path must explain the shortfall exactly like the
    // sequential path does; without this the skipped items simply vanish.
    let notices = recorder
        .messages()
        .into_iter()
        .filter(|msg| msg.contains("cancelled — stopping before next resource"))
        .count();
    assert_eq!(
        notices, 1,
        "cancellation should be announced exactly once, not per skipped item"
    );
}

#[test]
fn cancellation_before_dispatch_performs_no_work() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));
    let token = CancellationToken::new();
    token.cancel();
    let ctx = ctx.with_cancellation(token);

    let resources: Vec<FailingResource> = (0..BATCH)
        .map(|idx| FailingResource::new(format!("resource-{idx}"), FailAt::Always))
        .collect();

    let result = process_resources(&ctx, resources, &bail_opts())
        .expect("an already-cancelled run should short-circuit cleanly");

    let (changed, _already_ok, failed) = stats(&result);
    assert_eq!(changed, 0);
    assert_eq!(failed, 0);
}

// ---------------------------------------------------------------------------
// Logging under concurrency
// ---------------------------------------------------------------------------

#[test]
fn parallel_apply_emits_one_message_per_resource_without_dropping_any() {
    let (ctx, _log) = parallel_context(empty_config("/dotfiles".into()));
    let recorder = Arc::new(RecordingLog::default());
    let log: Arc<dyn crate::infra::logging::Log> = Arc::<RecordingLog>::clone(&recorder);
    let ctx = ctx.with_log(log);

    let resources: Vec<FailingResource> = (0..BATCH)
        .map(|idx| FailingResource::new(format!("resource-{idx:03}"), FailAt::Never))
        .collect();

    let result = process_resources(&ctx, resources, &ProcessOpts::lenient("install"))
        .expect("parallel apply should succeed");
    let (changed, _already_ok, failed) = stats(&result);
    assert_eq!(changed, u32::try_from(BATCH).expect("batch fits in u32"));
    assert_eq!(failed, 0);

    let messages = recorder.messages();
    for idx in 0..BATCH {
        let needle = format!("resource-{idx:03}");
        assert!(
            messages.iter().any(|msg| msg.contains(&needle)),
            "no message mentioned {needle}"
        );
    }
}
