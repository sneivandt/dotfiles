//! A [`Log`] test double that records everything it is given.
//!
//! Several test modules had grown their own near-identical recording logger.
//! This one is shared so assertions about console output and recorded task
//! results are written the same way everywhere.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};

use crate::infra::logging::{Log, MsgKind, Output, TaskRecorder, TaskStatus};

/// A single emitted message and its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedMessage {
    /// Message severity/presentation kind.
    pub kind: MsgKind,
    /// Rendered message text.
    pub text: String,
}

/// [`Log`] implementation that captures emitted messages in memory.
#[derive(Debug, Default)]
pub struct RecordingLog {
    messages: Mutex<Vec<RecordedMessage>>,
}

impl RecordingLog {
    /// Every message emitted so far, in order.
    #[must_use]
    pub fn messages(&self) -> Vec<RecordedMessage> {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Text of every message emitted with `kind`, in order.
    #[must_use]
    pub fn messages_of(&self, kind: MsgKind) -> Vec<String> {
        self.messages()
            .into_iter()
            .filter(|message| message.kind == kind)
            .map(|message| message.text)
            .collect()
    }

    /// Whether any message of `kind` contains `needle`.
    #[must_use]
    pub fn has_message(&self, kind: MsgKind, needle: &str) -> bool {
        self.messages_of(kind)
            .iter()
            .any(|text| text.contains(needle))
    }

    /// All emitted text joined by newlines, for coarse `contains` assertions.
    #[must_use]
    pub fn all_text(&self) -> String {
        self.messages()
            .into_iter()
            .map(|message| message.text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Output for RecordingLog {
    fn emit(&self, kind: MsgKind, msg: Cow<'_, str>) {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedMessage {
                kind,
                text: msg.into_owned(),
            });
    }
}

impl TaskRecorder for RecordingLog {
    /// Task recording is a no-op: the scheduler records task results, so a
    /// double used to drive a single task directly never sees them.
    fn record_task(&self, _name: &str, _status: TaskStatus, _message: Option<&str>) {}
}

/// Build a shared [`RecordingLog`] and the `Arc<dyn Log>` handle for a context.
#[must_use]
pub fn recording_log() -> (Arc<RecordingLog>, Arc<dyn Log>) {
    let log = Arc::new(RecordingLog::default());
    let handle: Arc<dyn Log> = Arc::<RecordingLog>::clone(&log);
    (log, handle)
}
