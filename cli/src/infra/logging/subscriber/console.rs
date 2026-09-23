//! Raw tracing diagnostics share the logger's console writer.
use std::sync::Arc;

use super::event::MessageExtractor;
use crate::infra::logging::console::Console;
use crate::infra::logging::types::MsgKind;

pub(in crate::infra::logging) struct ConsoleLayer {
    console: Arc<Console>,
}

impl ConsoleLayer {
    pub(in crate::infra::logging) const fn new(console: Arc<Console>) -> Self {
        Self { console }
    }
}

const fn message_kind(level: tracing::Level) -> Option<MsgKind> {
    match level {
        tracing::Level::ERROR => Some(MsgKind::Error),
        tracing::Level::WARN => Some(MsgKind::Warn),
        tracing::Level::INFO => Some(MsgKind::Info),
        tracing::Level::DEBUG | tracing::Level::TRACE => None,
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ConsoleLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if let Some(kind) = message_kind(*event.metadata().level()) {
            let mut extractor = MessageExtractor::default();
            event.record(&mut extractor);
            self.console.emit(kind, &extractor.message);
        }
    }
}

#[cfg(test)]
pub(super) fn console_line_with_style(
    level: tracing::Level,
    _target: &str,
    msg: &str,
    style: crate::infra::logging::style::StyleChoice,
    verbose: bool,
) -> Option<String> {
    crate::infra::logging::console::ui_line_with_style(message_kind(level)?, msg, style, verbose)
}
