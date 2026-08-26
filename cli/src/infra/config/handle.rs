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

    /// Return a cheap clone of the startup snapshot.
    #[must_use]
    pub fn read(&self) -> Arc<T> {
        Arc::clone(&self.inner)
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
}
