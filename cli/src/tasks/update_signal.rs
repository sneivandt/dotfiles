//! Typed signal shared between [`super::update::UpdateRepository`] and
//! [`super::reload_config::ReloadConfig`].
//!
//! `UpdateSignal` wraps an `Arc<AtomicBool>` but exposes only the two
//! operations that matter for this use-case: [`UpdateSignal::mark_updated`]
//! (called by `UpdateRepository`) and [`UpdateSignal::was_updated`] (called
//! by `ReloadConfig`).  This makes the cross-task coupling explicit and
//! self-documenting while remaining zero-cost at runtime.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A lightweight, cheaply-clonable flag that records whether the dotfiles
/// repository was updated during the current run.
///
/// Create one instance with [`UpdateSignal::new`] and clone it for each task
/// that needs access to the same flag.
#[derive(Debug, Clone)]
pub struct UpdateSignal {
    updated: Arc<AtomicBool>,
}

impl UpdateSignal {
    /// Create a new signal in the "not updated" state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            updated: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Record that the repository was updated.
    ///
    /// Called by [`super::update::UpdateRepository`] after a successful
    /// `git pull` that fetched new commits.
    pub fn mark_updated(&self) {
        self.updated.store(true, Ordering::Release);
    }

    /// Returns `true` if [`Self::mark_updated`] has been called.
    ///
    /// Called by [`super::reload_config::ReloadConfig`] to decide whether
    /// a config reload is necessary.
    #[must_use]
    pub fn was_updated(&self) -> bool {
        self.updated.load(Ordering::Acquire)
    }
}

impl Default for UpdateSignal {
    fn default() -> Self {
        Self::new()
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
