//! Typed signal shared between the repository-update task and the
//! configuration-reload task.
//!
//! `UpdateSignal` wraps an `AtomicFlag` but
//! exposes only the two operations that matter for this use-case:
//! [`UpdateSignal::mark_updated`](crate::engine::update_signal::UpdateSignal::mark_updated)
//! (called after a successful pull) and
//! [`UpdateSignal::was_updated`](crate::engine::update_signal::UpdateSignal::was_updated)
//! (called to decide whether a reload is necessary).  This makes the cross-task
//! coupling explicit and self-documenting while remaining zero-cost at runtime.

use crate::infra::atomic_flag::AtomicFlag;

/// A lightweight, cheaply-clonable flag that records whether the dotfiles
/// repository was updated during the current run.
///
/// Create one instance with [`UpdateSignal::new`] and clone it for each task
/// that needs access to the same flag.
#[derive(Debug, Clone, Default)]
pub struct UpdateSignal {
    flag: AtomicFlag,
}

impl UpdateSignal {
    /// Create a new signal in the "not updated" state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that the repository was updated.
    ///
    /// Called by the repository-update task after a successful `git pull` that
    /// fetched new commits.
    pub fn mark_updated(&self) {
        self.flag.set();
    }

    /// Returns `true` if [`Self::mark_updated`] has been called.
    ///
    /// Called by the configuration-reload task to decide whether a config
    /// reload is necessary.
    #[must_use]
    pub fn was_updated(&self) -> bool {
        self.flag.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_signal_is_not_updated() {
        assert!(!UpdateSignal::new().was_updated());
    }

    #[test]
    fn mark_updated_sets_flag() {
        let sig = UpdateSignal::new();
        sig.mark_updated();
        assert!(sig.was_updated());
    }

    #[test]
    fn clone_sees_same_state() {
        let sig = UpdateSignal::new();
        let cloned = sig.clone();
        sig.mark_updated();
        assert!(cloned.was_updated());
    }
}
