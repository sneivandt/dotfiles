//! Run-local exclusive access, independent of prerequisite ordering.

use std::collections::HashSet;
use std::ffi::OsString;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::CancellationToken;

/// Exclusive access to shared state for one complete work-item lifecycle.
///
/// Equal namespace/key pairs conflict. Keys must identify the shared state,
/// not the setting being edited. Claims neither order work nor propagate
/// another holder's failure, and do not lock out external processes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccessClaim {
    namespace: &'static str,
    key: OsString,
}

impl AccessClaim {
    /// Identify state requiring exclusive access within this CLI run.
    #[must_use]
    pub fn exclusive(namespace: &'static str, key: impl Into<OsString>) -> Self {
        Self {
            namespace,
            key: key.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("cancelled while waiting for exclusive access")]
pub(super) struct AccessCancelled;

#[derive(Debug, Default)]
pub(crate) struct AccessRegistry {
    held: Mutex<HashSet<AccessClaim>>,
    changed: Condvar,
}

impl AccessRegistry {
    pub(crate) fn try_acquire(self: &Arc<Self>, claims: &[AccessClaim]) -> Option<AccessGuard> {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if claims.iter().any(|claim| held.contains(claim)) {
            return None;
        }
        held.extend(claims.iter().cloned());
        Some(AccessGuard {
            registry: Arc::clone(self),
            claims: claims.to_vec(),
        })
    }

    pub(crate) fn acquire(
        self: &Arc<Self>,
        claims: &[AccessClaim],
        cancelled: &CancellationToken,
    ) -> Result<AccessGuard, AccessCancelled> {
        if claims.is_empty() {
            return Ok(AccessGuard {
                registry: Arc::clone(self),
                claims: Vec::new(),
            });
        }
        loop {
            if cancelled.is_cancelled() {
                return Err(AccessCancelled);
            }
            if let Some(guard) = self.try_acquire(claims) {
                return Ok(guard);
            }
            self.wait(cancelled)?;
        }
    }

    pub(crate) fn wait(&self, cancelled: &CancellationToken) -> Result<(), AccessCancelled> {
        if cancelled.is_cancelled() {
            return Err(AccessCancelled);
        }
        let held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Cancellation is signal-safe and cannot notify this condition variable.
        // A bounded wait also closes the notify-before-wait race.
        let _wait = self
            .changed
            .wait_timeout(held, Duration::from_millis(25))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct AccessGuard {
    registry: Arc<AccessRegistry>,
    claims: Vec<AccessClaim>,
}

impl Drop for AccessGuard {
    fn drop(&mut self) {
        let mut held = self
            .registry
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for claim in &self.claims {
            held.remove(claim);
        }
        self.registry.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(key: &str) -> AccessClaim {
        AccessClaim::exclusive("fixture", key)
    }

    #[test]
    fn acquisition_is_atomic_deduplicated_and_order_independent() {
        let registry = Arc::new(AccessRegistry::default());
        let holder = registry.try_acquire(&[claim("a"), claim("a")]).unwrap();
        assert!(registry.try_acquire(&[claim("b"), claim("a")]).is_none());
        let independent = registry.try_acquire(&[claim("b")]).unwrap();
        assert!(registry.try_acquire(&[claim("a"), claim("b")]).is_none());
        drop((holder, independent));
        let both = registry.try_acquire(&[claim("b"), claim("a")]).unwrap();
        assert!(registry.try_acquire(&[claim("a")]).is_none());
        drop(both);
        assert!(registry.try_acquire(&[claim("a"), claim("b")]).is_some());
    }

    #[test]
    fn namespaces_and_runs_are_independent() {
        let registry = Arc::new(AccessRegistry::default());
        let _holder = registry.try_acquire(&[claim("document")]).unwrap();
        let other_namespace = AccessClaim::exclusive("another domain", "document");
        assert!(registry.try_acquire(&[other_namespace]).is_some());
        let other_run = Arc::new(AccessRegistry::default());
        assert!(other_run.try_acquire(&[claim("document")]).is_some());
    }

    #[test]
    fn cancelled_waits_fail_without_reserving_access() {
        let registry = Arc::new(AccessRegistry::default());
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(registry.wait(&cancelled).is_err());
        assert!(registry.acquire(&[claim("document")], &cancelled).is_err());
        assert!(registry.try_acquire(&[claim("document")]).is_some());
    }

    #[test]
    fn unwinding_releases_claims() {
        let registry = Arc::new(AccessRegistry::default());
        let result = std::panic::catch_unwind(|| {
            let _guard = registry.try_acquire(&[claim("document")]).unwrap();
            panic!("fixture panic");
        });
        assert!(result.is_err());
        assert!(registry.try_acquire(&[claim("document")]).is_some());
    }
}
