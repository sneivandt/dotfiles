//! Generic, atomically-swappable typed configuration handle.
//!
//! [`ConfigHandle<T>`] wraps a single piece of configuration data behind an
//! `Arc<RwLock<Arc<T>>>` so that many holders can share a cheap, cloneable
//! reference to the *same* slot.  Reads return an `Arc<T>` snapshot (the lock
//! is held only for the duration of the `Arc::clone`), and a writer can swap
//! in fresh data that every holder sees on its next read.
//!
//! This is the mechanism the application layer uses to give each concrete task
//! a handle to *only* the slice of configuration it needs, without any task
//! depending on the aggregate `Config` type.  During an app-owned reload, the
//! application swaps each handle in place; because every task holds a clone of
//! the same handle, the update is visible without rebuilding tasks.

use std::sync::{Arc, PoisonError, RwLock};

/// A shared, atomically-swappable handle to a single piece of configuration.
///
/// Cloning a `ConfigHandle` is cheap (an `Arc` bump) and all clones observe the
/// same underlying slot, so a [`swap`](ConfigHandle::swap) performed through one
/// clone is visible through every other.
pub struct ConfigHandle<T> {
    inner: Arc<RwLock<Arc<T>>>,
}

impl<T> ConfigHandle<T> {
    /// Create a new handle wrapping `value`.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Arc::new(value))),
        }
    }

    /// Return a cheap snapshot of the current value.
    ///
    /// The read lock is held only long enough to clone the inner `Arc`, so the
    /// returned snapshot can be held for as long as needed without blocking a
    /// concurrent [`swap`](ConfigHandle::swap).  A poisoned lock is recovered
    /// transparently — configuration data is immutable behind the `Arc`, so a
    /// panic elsewhere cannot leave it half-written.
    #[must_use]
    pub fn read(&self) -> Arc<T> {
        Arc::clone(&self.inner.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Atomically replace the stored value.
    ///
    /// Every clone of this handle observes `value` on its next
    /// [`read`](ConfigHandle::read).
    pub fn swap(&self, value: T) {
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        *guard = Arc::new(value);
    }
}

impl<T> Clone for ConfigHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> std::fmt::Debug for ConfigHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigHandle").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_initial_value() {
        let handle = ConfigHandle::new(vec![1, 2, 3]);
        assert_eq!(*handle.read(), vec![1, 2, 3]);
    }

    #[test]
    fn swap_is_visible_through_clones() {
        let handle = ConfigHandle::new(1u32);
        let clone = handle.clone();
        handle.swap(42);
        assert_eq!(*clone.read(), 42);
    }

    #[test]
    fn snapshot_is_stable_across_swap() {
        let handle = ConfigHandle::new(vec![1]);
        let snapshot = handle.read();
        handle.swap(vec![2]);
        assert_eq!(*snapshot, vec![1]);
        assert_eq!(*handle.read(), vec![2]);
    }
}
