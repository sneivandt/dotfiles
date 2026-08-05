//! Core resource traits and lifecycle result types.

use std::result::Result;

use super::ResourceError;

/// Result type returned by [`Resource`] operations (`apply`/`remove`) and by
/// resource state discovery.
///
/// The error half is the typed [`ResourceError`], so the orchestration layer
/// can classify failures (see [`ResourceError::category`]) without
/// downcasting. Helper errors that have no dedicated variant flow through
/// [`ResourceError::Other`], which preserves their context chain and still
/// recovers the original category on round-trip.
pub type ResourceResult<T> = Result<T, ResourceError>;

/// Interface for resources that can be described, applied, and removed.
///
/// State discovery is intentionally separate from this trait. The engine uses
/// a [`ResourceStateProvider`](super::ResourceStateProvider) to determine
/// whether each resource is already in the desired state, which allows both
/// intrinsic per-resource checks and cached/bulk checks to share the same
/// orchestration path.
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
}

/// A [`Resource`] that can also be removed, undoing a previous `apply()`.
///
/// Removability is a property of the resource type, not of a particular run, so
/// it is expressed as a separate trait rather than a defaulted method that
/// fails at runtime. The removal entry points in the orchestration layer
/// require this bound, which makes "this resource cannot be removed" a
/// compile error at the call site instead of a failed task.
pub trait RemovableResource: Resource {
    /// Remove the resource, undoing a previous `apply()`.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] if the resource cannot be removed due to I/O
    /// failures, permission issues, or other system errors.
    fn remove(&self) -> ResourceResult<ResourceChange>;
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
    /// is. The processing engine skips `Unknown` resources rather than applying
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
/// let skipped = ResourceChange::skipped("source missing");
/// let unusable = ResourceChange::unusable("failed to install");
///
/// assert_eq!(applied, ResourceChange::Applied);
/// assert_ne!(applied, noop);
/// assert_ne!(skipped, unusable);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceChange {
    /// Resource was created or updated.
    Applied,
    /// Resource was already correct (no change needed).
    AlreadyCorrect,
    /// Resource was skipped without applying a change (e.g., missing source
    /// file, unavailable tool, or protected target directory).
    ///
    /// `failed` mirrors [`ApplyOperation::Skip`] in the planning layer: a skip
    /// decided at apply time is not automatically a failure. Deliberate,
    /// benign skips (an unsupported platform, or refusing to overwrite user
    /// data) set it to `false`; skips that mean the work could not be done set
    /// it to `true` so the run summary counts them.
    ///
    /// [`ApplyOperation::Skip`]: crate::engine::ApplyOperation::Skip
    Skipped {
        /// Reason why the resource was skipped.
        reason: String,
        /// Whether the skip should be counted as a failed item.
        failed: bool,
    },
}

impl ResourceChange {
    /// A benign skip that must not be counted as a failure.
    ///
    /// Use this when the resource deliberately did nothing — the platform does
    /// not support it, or applying would destroy user data.
    #[must_use]
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            failed: false,
        }
    }

    /// A skip that means the work could not be done and should be counted as a
    /// failed item.
    ///
    /// Use this when the resource tried and the underlying tool refused, so the
    /// run summary reports the item rather than silently passing.
    #[must_use]
    pub fn unusable(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
            failed: true,
        }
    }
}
