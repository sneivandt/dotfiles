//! Thread-safe output capture shared by unit tests.

use std::borrow::Cow;
use std::sync::Mutex;

use crate::infra::logging::{Logger, MsgKind, Output, RunLog, TaskEntry, TaskRecorder};

/// Records messages by kind and completed task entries without console output.
#[derive(Debug, Default)]
pub struct CapturingOutput {
    messages: Mutex<Vec<(MsgKind, String)>>,
    records: Mutex<Vec<TaskEntry>>,
    logger: Option<Logger>,
}

impl CapturingOutput {
    /// Retain an isolated logger so failure-reporting tests can inspect log hints.
    pub fn with_logger(logger: Logger) -> Self {
        Self {
            logger: Some(logger),
            ..Self::default()
        }
    }

    /// Snapshot every emitted message in emission order.
    pub fn messages(&self) -> Vec<(MsgKind, String)> {
        self.messages.lock().unwrap().clone()
    }

    /// Snapshot messages of one kind in emission order.
    pub fn messages_of(&self, kind: MsgKind) -> Vec<String> {
        self.messages()
            .into_iter()
            .filter_map(|(actual, text)| (actual == kind).then_some(text))
            .collect()
    }

    /// Snapshot completed task entries in recording order.
    pub fn records(&self) -> Vec<TaskEntry> {
        self.records.lock().unwrap().clone()
    }
}

impl Output for CapturingOutput {
    fn run_log(&self) -> Option<&RunLog> {
        self.logger.as_ref().and_then(Output::run_log)
    }

    fn emit(&self, kind: MsgKind, message: Cow<'_, str>) {
        self.messages
            .lock()
            .unwrap()
            .push((kind, message.into_owned()));
    }
}

impl TaskRecorder for CapturingOutput {
    fn record_task(&self, task: TaskEntry) {
        self.records.lock().unwrap().push(task);
    }
}
