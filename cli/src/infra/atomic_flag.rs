//! Shared one-shot boolean flag backing cross-thread signalling primitives.
//!
//! [`AtomicFlag`] wraps an `Arc<AtomicBool>` and is the common implementation
//! behind the cancellation and update-signal primitives.  It is cheaply
//! clonable; all clones share the same underlying flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A lightweight, cheaply-clonable flag shared across threads.
///
/// All clones observe the same underlying `AtomicBool`, so a [`set`](Self::set)
/// on any clone is visible to every other clone via [`get`](Self::get).
#[derive(Debug, Clone)]
pub(crate) struct AtomicFlag {
    flag: Arc<AtomicBool>,
}

impl AtomicFlag {
    /// Create a new flag in the "unset" (`false`) state.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the flag to `true` with [`Ordering::Release`].
    pub(crate) fn set(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// Returns `true` if the flag has been set, reading with [`Ordering::Acquire`].
    #[must_use]
    pub(crate) fn get(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

impl Default for AtomicFlag {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_flag_is_unset() {
        assert!(!AtomicFlag::new().get());
    }

    #[test]
    fn set_marks_flag() {
        let flag = AtomicFlag::new();
        flag.set();
        assert!(flag.get());
    }

    #[test]
    fn clone_shares_state() {
        let flag = AtomicFlag::new();
        let cloned = flag.clone();
        flag.set();
        assert!(cloned.get());
    }
}
