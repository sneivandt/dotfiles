//! Generic typed configuration handles backed by immutable snapshots.
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

type Reader<T> = dyn Fn() -> Arc<T> + Send + Sync;

enum ConfigHandleInner<T> {
    Standalone(Arc<RwLock<Arc<T>>>),
    Projection(Arc<Reader<T>>),
}

/// A shared, atomically-swappable handle to a single piece of configuration.
///
/// Cloning a `ConfigHandle` is cheap (an `Arc` bump) and all clones observe the
/// same underlying slot, so a [`swap`](ConfigHandle::swap) performed through one
/// clone is visible through every other.
pub struct ConfigHandle<T> {
    inner: ConfigHandleInner<T>,
}

impl<T> ConfigHandle<T> {
    /// Create a new handle wrapping `value`.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            inner: ConfigHandleInner::Standalone(Arc::new(RwLock::new(Arc::new(value)))),
        }
    }

    /// Return a cheap snapshot of the current value.
    ///
    /// The read lock is held only long enough to clone the inner `Arc`, so the
    /// returned snapshot can be held for as long as needed without blocking a
    /// concurrent source publication. A poisoned lock is recovered
    /// transparently — configuration data is immutable behind the `Arc`, so a
    /// panic elsewhere cannot leave it half-written.
    #[must_use]
    pub fn read(&self) -> Arc<T> {
        match &self.inner {
            ConfigHandleInner::Standalone(inner) => {
                Arc::clone(&inner.read().unwrap_or_else(PoisonError::into_inner))
            }
            ConfigHandleInner::Projection(read) => read(),
        }
    }
}

impl<T> Clone for ConfigHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: match &self.inner {
                ConfigHandleInner::Standalone(inner) => {
                    ConfigHandleInner::Standalone(Arc::clone(inner))
                }
                ConfigHandleInner::Projection(read) => {
                    ConfigHandleInner::Projection(Arc::clone(read))
                }
            },
        }
    }
}

/// One atomically published immutable configuration generation.
pub(crate) struct ConfigSource<T> {
    inner: Arc<RwLock<Arc<T>>>,
}

impl<T> ConfigSource<T> {
    #[must_use]
    pub(crate) fn new(value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Arc::new(value))),
        }
    }

    pub(crate) fn swap(&self, value: T) {
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        *guard = Arc::new(value);
    }

    pub(crate) fn project<U>(&self, project: fn(&T) -> U) -> ConfigHandle<U>
    where
        T: Send + Sync + 'static,
        U: Send + Sync + 'static,
    {
        let source = Arc::clone(&self.inner);
        ConfigHandle {
            inner: ConfigHandleInner::Projection(Arc::new(move || {
                let snapshot = Arc::clone(&source.read().unwrap_or_else(PoisonError::into_inner));
                Arc::new(project(&snapshot))
            })),
        }
    }
}

impl<T> Clone for ConfigSource<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> std::fmt::Debug for ConfigSource<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigSource").finish_non_exhaustive()
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
    fn source_swap_is_visible_through_projected_handles() {
        let source = ConfigSource::new((1_u32, 2_u32));
        let first = source.project(|value| value.0);
        let second = source.project(|value| value.1);
        source.swap((42, 84));
        assert_eq!(*first.read(), 42);
        assert_eq!(*second.read(), 84);
    }

    #[test]
    fn snapshot_is_stable_across_swap() {
        let source = ConfigSource::new(vec![1]);
        let handle = source.project(Clone::clone);
        let snapshot = handle.read();
        source.swap(vec![2]);
        assert_eq!(*snapshot, vec![1]);
        assert_eq!(*handle.read(), vec![2]);
    }
}
