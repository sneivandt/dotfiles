//! Shared tracing event field extraction.

/// Extracts the `message` field from a [`tracing::Event`].
#[derive(Default)]
pub(super) struct MessageExtractor {
    pub(super) message: String,
}

impl tracing::field::Visit for MessageExtractor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}
