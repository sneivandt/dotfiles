//! Bridge from raw [`tracing`] events into the [`RunLog`].
//!
//! `Logger` writes its own messages straight to the run log, so this layer
//! exists only to capture `tracing::debug!` / `warn!` calls made directly by
//! `infra` and `domains` code.  Those events carry no task span, so context is
//! taken from the run log's thread-local task name — the same mechanism the
//! parallel scheduler already uses.

use std::sync::Arc;

use super::event::MessageExtractor;
use crate::infra::logging::runlog::RunLog;
use crate::infra::logging::types::LogEvent;

/// Target prefix used by `Logger` for console-bound events.
///
/// Events under this prefix are already recorded in the run log by `Logger`
/// itself, so the bridge skips them to avoid writing each message twice.
pub(in crate::infra::logging) const UI_TARGET_PREFIX: &str = "dotfiles::ui";

/// A [`tracing_subscriber::Layer`] that forwards non-UI events to the run log.
#[derive(Debug)]
pub(in crate::infra::logging) struct RunLogLayer {
    run_log: Arc<RunLog>,
}

impl RunLogLayer {
    /// Create a layer that writes into the given run log.
    pub(in crate::infra::logging) const fn new(run_log: Arc<RunLog>) -> Self {
        Self { run_log }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for RunLogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        if metadata.target().starts_with(UI_TARGET_PREFIX) {
            return;
        }

        let mut extractor = MessageExtractor::default();
        event.record(&mut extractor);
        self.run_log
            .emit(LogEvent::from_level(*metadata.level()), &extractor.message);
    }
}
