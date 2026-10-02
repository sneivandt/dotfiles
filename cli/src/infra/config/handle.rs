//! Generic typed handles for immutable configuration snapshots.

use std::sync::Arc;

/// A shared immutable handle to a single piece of configuration.
///
/// Cloning a `ConfigHandle` is cheap and every clone references the same
/// startup snapshot.
pub struct ConfigHandle<T> {
    inner: Arc<T>,
}

impl<T> ConfigHandle<T> {
    /// Create a new handle wrapping `value`.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(value),
        }
    }

    /// Borrow the immutable startup snapshot.
    #[must_use]
    pub fn get(&self) -> &T {
        &self.inner
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
    fn cloned_handles_share_the_startup_snapshot_without_cloning_the_value() {
        struct NotClone;
        let handle = ConfigHandle::new(NotClone);
        assert!(std::ptr::eq(handle.get(), handle.get()));
        let cloned = handle.clone();
        assert!(std::ptr::eq(handle.get(), cloned.get()));
    }
}
