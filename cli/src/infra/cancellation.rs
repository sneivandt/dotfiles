//! Process-wide cancellation flag for graceful shutdown.
//!
//! `CancellationToken` wraps an [`AtomicFlag`](AtomicFlag)
//! and exposes only the two operations needed for cooperative cancellation:
//! [`CancellationToken::cancel`] (called by the signal handler) and
//! [`CancellationToken::is_cancelled`] (polled by processing loops before each
//! work item).

use super::atomic_flag::AtomicFlag;

/// A lightweight, cheaply-clonable flag that records whether the process
/// has been asked to shut down (e.g. via Ctrl-C).
///
/// Create one instance with [`CancellationToken::new`] and clone it for the
/// signal handler and the execution [`Context`](super::Context).
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    flag: AtomicFlag,
}

impl CancellationToken {
    /// Create a new token in the "not cancelled" state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Signal cancellation.
    ///
    /// Called from the `ctrlc` handler when the user presses Ctrl-C.
    pub fn cancel(&self) {
        self.flag.set();
    }

    /// Returns `true` if [`Self::cancel`] has been called.
    ///
    /// Polled by processing loops to decide whether to stop dispatching
    /// new work items.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_propagates_across_threads_without_affecting_independent_tokens() {
        let token = CancellationToken::new();
        let independent = CancellationToken::new();
        let cloned = token.clone();
        assert!(!token.is_cancelled());
        assert!(!cloned.is_cancelled());
        std::thread::spawn(move || {
            cloned.cancel();
            cloned.cancel();
            assert!(cloned.is_cancelled(), "repeated cancellation remains set");
        })
        .join()
        .unwrap();
        assert!(
            token.is_cancelled(),
            "the original observes worker cancellation"
        );
        assert!(
            !independent.is_cancelled(),
            "tokens from distinct runs are isolated"
        );
    }
}
