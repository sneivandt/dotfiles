//! Rayon-based parallel resource processing.

use anyhow::Result;

use super::batch::BatchProgress;
use super::context::Context;
use super::stats::{TaskResult, TaskStats};
use crate::infra::logging::OutputExt as _;
use crate::infra::logging::{log_thread_name, set_log_thread_name};

/// Accumulate per-item [`TaskStats`] deltas in parallel using Rayon.
///
/// Each worker retains its outcomes even if another fails. A local stop flag
/// prevents new dispatch after an error without cancelling independent tasks;
/// work already in flight is joined and included in the final report.
///
/// The diagnostic thread name is captured once before dispatching and re-set
/// on each iteration so the log timeline remains accurate even when Rayon
/// reuses threads across work items (a stale name is harmless but misleading).
///
/// When the run is cancelled, remaining items are skipped so that in-flight
/// operations can finish cleanly.  The same notice the sequential path prints
/// is emitted once, so an interrupted parallel run explains the shortfall
/// between the items it reports and the items it was given rather than
/// appearing to have silently processed fewer resources.
pub(super) fn collect_parallel_stats<T: Send>(
    ctx: &Context,
    items: Vec<T>,
    work: impl Fn(T) -> Result<TaskStats> + Sync + Send,
) -> Result<TaskResult> {
    use rayon::prelude::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    let task_name = log_thread_name();
    let cancel_notice = std::sync::Once::new();
    let stopped = AtomicBool::new(false);
    items
        .into_par_iter()
        .fold(BatchProgress::default, |mut progress, item| {
            set_log_thread_name(&task_name);
            if ctx.is_cancelled() {
                cancel_notice.call_once(|| {
                    ctx.log().warn("cancelled — stopping before next resource");
                });
                progress.omit(1);
            } else if stopped.load(Ordering::Acquire) {
                progress.omit(1);
            } else if progress.record(work(item)) {
                stopped.store(true, Ordering::Release);
            }
            progress
        })
        .reduce(BatchProgress::default, BatchProgress::merge)
        .finish()
}
