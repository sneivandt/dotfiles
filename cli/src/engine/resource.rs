//! Generic resource contract: the idempotent check + apply primitives shared
//! by all concrete domain resources.

use anyhow::Result;

mod error;

pub use error::ResourceError;

/// Result type returned by [`Resource`] operations (`apply`/`remove`).
///
/// Unlike the `anyhow::Result` used for state discovery, the error half is the
/// typed [`ResourceError`], so the orchestration layer can classify failures
/// (see [`ResourceError::category`]) without downcasting.  Helper errors that
/// have no dedicated variant flow through [`ResourceError::Other`].
pub type ResourceResult<T> = std::result::Result<T, ResourceError>;

/// Interface for resources that can be described, applied, and removed.
///
/// State discovery is intentionally separate from this trait.  The engine uses
/// a [`ResourceStateProvider`] to determine whether each resource is already in
/// the desired state, which allows both intrinsic per-resource checks and
/// cached/bulk checks to share the same orchestration path.
pub trait Resource {
    /// Human-readable description of this resource.
    fn description(&self) -> String;

    /// Return a user-visible warning that must be emitted immediately before
    /// applying this resource.
    ///
    /// Resources should override this when applying the current state can
    /// irreversibly discard user data. The default is no warning.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] if the resource cannot safely determine
    /// whether applying it is destructive.
    fn pre_apply_warning(&self) -> ResourceResult<Option<String>> {
        Ok(None)
    }

    /// Apply the resource change.
    ///
    /// This method should:
    /// - Create parent directories if needed
    /// - Update the resource to match the desired state
    /// - Return the appropriate `ResourceChange` result
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] if the resource cannot be applied due to I/O
    /// failures, permission issues, invalid paths, or other system errors.
    fn apply(&self) -> ResourceResult<ResourceChange>;

    /// Remove the resource, undoing a previous `apply()`.
    ///
    /// Default implementation returns an error — override in resources
    /// that support removal.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] if the resource cannot be removed, or if
    /// removal is not supported for this resource type.
    fn remove(&self) -> ResourceResult<ResourceChange> {
        Err(ResourceError::not_supported(format!(
            "operation 'remove' is not supported for resource '{}'",
            self.description()
        )))
    }
}

/// State of a resource (file, registry entry, etc.).
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::resources::ResourceState;
///
/// let missing = ResourceState::Missing;
/// let correct = ResourceState::Correct;
/// let wrong = ResourceState::Incorrect { current: "/other/path".into() };
/// let skip = ResourceState::Invalid { reason: "target is a directory".into() };
/// let unknown = ResourceState::Unknown { reason: "SHELL not set".into() };
///
/// assert_ne!(missing, correct);
/// assert_eq!(correct, ResourceState::Correct);
/// assert_ne!(unknown, missing);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceState {
    /// Resource does not exist or is not present.
    Missing,
    /// Resource exists and matches the desired state.
    Correct,
    /// Resource exists but does not match the desired state.
    Incorrect {
        /// The current value of the resource.
        current: String,
    },
    /// Resource cannot be applied (e.g., target is a directory that shouldn't be removed).
    Invalid {
        /// Reason why the resource cannot be applied.
        reason: String,
    },
    /// Resource state cannot be determined (e.g., detection tool unavailable).
    ///
    /// Unlike [`Missing`], this variant does not imply the resource needs to be
    /// created — it means the engine genuinely cannot tell what the current state
    /// is.  The processing engine skips `Unknown` resources rather than applying
    /// them, and logs the reason so the operator can investigate.
    ///
    /// [`Missing`]: Self::Missing
    Unknown {
        /// Reason why the state could not be determined.
        reason: String,
    },
}

impl std::fmt::Display for ResourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "missing"),
            Self::Correct => write!(f, "correct"),
            Self::Incorrect { current } => write!(f, "incorrect (current: {current})"),
            Self::Invalid { reason } => write!(f, "invalid ({reason})"),
            Self::Unknown { reason } => write!(f, "unknown ({reason})"),
        }
    }
}

/// Result of applying a resource change.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::resources::ResourceChange;
///
/// let applied = ResourceChange::Applied;
/// let noop = ResourceChange::AlreadyCorrect;
/// let skipped = ResourceChange::Skipped { reason: "source missing".into() };
///
/// assert_eq!(applied, ResourceChange::Applied);
/// assert_ne!(applied, noop);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceChange {
    /// Resource was created or updated.
    Applied,
    /// Resource was already correct (no change needed).
    AlreadyCorrect,
    /// Resource was skipped without applying a change (e.g., missing source
    /// file, unavailable tool, or protected target directory).
    Skipped {
        /// Reason why the resource was skipped.
        reason: String,
    },
}

/// Provides current state for a batch of resources.
///
/// Implementations may either use no cache (for intrinsic checks) or load a
/// shared cache once and reuse it for every resource in the batch.
pub trait ResourceStateProvider<R: Resource> {
    /// Cached state shared across all resources in this batch.
    type Cache: Sync;

    /// Load shared state for this batch.
    ///
    /// # Errors
    ///
    /// Returns an error if the state cache cannot be loaded.
    fn load(&self, resources: &[R]) -> Result<Self::Cache>;

    /// Determine the current state for one resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource state cannot be determined.
    fn current_state(&self, resource: &R, cache: &Self::Cache) -> Result<ResourceState>;
}

/// State-checking extension for resources that can inspect themselves.
///
/// This is bridged into the orchestration layer by [`IntrinsicStateProvider`].
pub trait IntrinsicState: Resource {
    /// Check the current state of the resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource state cannot be determined due to I/O failures,
    /// permission issues, or other system errors.
    fn current_state(&self) -> Result<ResourceState>;

    /// Determine if the resource needs to be changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the current state cannot be determined (propagates errors from
    /// `current_state()`).
    #[allow(dead_code, reason = "part of trait contract; used by test modules")]
    fn needs_change(&self) -> Result<bool> {
        Ok(matches!(
            self.current_state()?,
            ResourceState::Missing | ResourceState::Incorrect { .. }
        ))
    }
}

/// State provider for resources that implement [`IntrinsicState`].
#[derive(Debug, Clone, Copy, Default)]
pub struct IntrinsicStateProvider;

impl<R: IntrinsicState> ResourceStateProvider<R> for IntrinsicStateProvider {
    type Cache = ();

    fn load(&self, _resources: &[R]) -> Result<Self::Cache> {
        Ok(())
    }

    fn current_state(&self, resource: &R, _cache: &Self::Cache) -> Result<ResourceState> {
        resource.current_state()
    }
}

/// State provider backed by an already-loaded cache.
#[derive(Debug, Clone)]
pub struct PreloadedStateProvider<Cache, State> {
    cache: Cache,
    state: State,
}

/// State provider backed by a borrowed cache.
#[derive(Debug, Clone)]
pub struct BorrowedStateProvider<'cache, Cache: ?Sized, State> {
    cache: &'cache Cache,
    state: State,
}

impl<Cache, State> PreloadedStateProvider<Cache, State> {
    /// Create a provider from a cache value and state-mapping closure.
    #[must_use]
    pub const fn new(cache: Cache, state: State) -> Self {
        Self { cache, state }
    }
}

impl<'cache, Cache: ?Sized, State> BorrowedStateProvider<'cache, Cache, State> {
    /// Create a provider from a borrowed cache and state-mapping closure.
    #[must_use]
    pub const fn new(cache: &'cache Cache, state: State) -> Self {
        Self { cache, state }
    }
}

impl<R, Cache, State> ResourceStateProvider<R> for PreloadedStateProvider<Cache, State>
where
    R: Resource,
    Cache: Sync,
    State: Fn(&R, &Cache) -> Result<ResourceState> + Sync,
{
    type Cache = ();

    fn load(&self, _resources: &[R]) -> Result<Self::Cache> {
        Ok(())
    }

    fn current_state(&self, resource: &R, _cache: &Self::Cache) -> Result<ResourceState> {
        (self.state)(resource, &self.cache)
    }
}

impl<R, Cache, State> ResourceStateProvider<R> for BorrowedStateProvider<'_, Cache, State>
where
    R: Resource,
    Cache: Sync + ?Sized,
    State: Fn(&R, &Cache) -> Result<ResourceState> + Sync,
{
    type Cache = ();

    fn load(&self, _resources: &[R]) -> Result<Self::Cache> {
        Ok(())
    }

    fn current_state(&self, resource: &R, _cache: &Self::Cache) -> Result<ResourceState> {
        (self.state)(resource, self.cache)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    struct TestResource {
        state: ResourceState,
    }

    impl Resource for TestResource {
        fn description(&self) -> String {
            "test resource".to_string()
        }

        fn apply(&self) -> ResourceResult<ResourceChange> {
            Ok(ResourceChange::Applied)
        }
    }

    impl IntrinsicState for TestResource {
        fn current_state(&self) -> Result<ResourceState> {
            Ok(self.state.clone())
        }
    }

    #[test]
    fn needs_change_for_missing_resource() {
        let resource = TestResource {
            state: ResourceState::Missing,
        };
        assert!(resource.needs_change().unwrap());
    }

    #[test]
    fn needs_change_for_incorrect_resource() {
        let resource = TestResource {
            state: ResourceState::Incorrect {
                current: "wrong".to_string(),
            },
        };
        assert!(resource.needs_change().unwrap());
    }

    #[test]
    fn no_change_for_correct_resource() {
        let resource = TestResource {
            state: ResourceState::Correct,
        };
        assert!(!resource.needs_change().unwrap());
    }

    #[test]
    fn no_change_for_invalid_resource() {
        let resource = TestResource {
            state: ResourceState::Invalid {
                reason: "directory exists".to_string(),
            },
        };
        assert!(!resource.needs_change().unwrap());
    }

    #[test]
    fn no_change_for_unknown_resource() {
        let resource = TestResource {
            state: ResourceState::Unknown {
                reason: "detection tool unavailable".to_string(),
            },
        };
        assert!(!resource.needs_change().unwrap());
    }

    #[test]
    fn unknown_state_display() {
        let state = ResourceState::Unknown {
            reason: "env var not set".to_string(),
        };
        assert_eq!(state.to_string(), "unknown (env var not set)");
    }

    #[test]
    fn default_remove_returns_error() {
        let resource = TestResource {
            state: ResourceState::Correct,
        };
        let err = resource.remove().unwrap_err();
        assert!(
            err.to_string().contains("not supported"),
            "expected 'not supported' in: {err}"
        );
        assert!(
            err.to_string().contains("test resource"),
            "expected resource description in: {err}"
        );
    }
}
