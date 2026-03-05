//! Idempotent resource primitives (check + apply pattern).
pub mod chmod;
pub mod copilot_skill;
pub mod developer_mode;
pub mod git_config;
pub mod hook;
pub mod package;
pub mod registry;
pub mod shell;
pub mod symlink;
pub mod systemd_unit;
pub mod vscode_extension;

use anyhow::Result;

/// Minimal interface for resources that can be described, applied, and removed.
///
/// Resources whose state is determined via a single external bulk query (e.g.
/// VS Code extensions) should implement only this trait.  Resources that can
/// determine their own state independently implement the richer [`Resource`]
/// super-trait.
pub trait Applicable {
    /// Human-readable description of this resource.
    fn description(&self) -> String;

    /// Apply the resource change.
    ///
    /// This method should:
    /// - Create parent directories if needed
    /// - Update the resource to match the desired state
    /// - Return the appropriate `ResourceChange` result
    ///
    /// # Errors
    ///
    /// Returns an error if the resource cannot be applied due to I/O failures,
    /// permission issues, invalid paths, or other system errors.
    fn apply(&self) -> Result<ResourceChange>;

    /// Remove the resource, undoing a previous `apply()`.
    ///
    /// Default implementation returns an error — override in resources
    /// that support removal.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource cannot be removed, or if removal is not supported
    /// for this resource type.
    fn remove(&self) -> Result<ResourceChange> {
        Err(crate::error::ResourceError::NotSupported {
            reason: format!(
                "operation 'remove' is not supported for resource '{}'",
                self.description()
            ),
        }
        .into())
    }
}

/// State of a resource (file, registry entry, etc.).
///
/// # Examples
///
/// ```
/// use dotfiles_cli::resources::ResourceState;
///
/// let missing = ResourceState::Missing;
/// let correct = ResourceState::Correct;
/// let wrong = ResourceState::Incorrect { current: "/other/path".into() };
/// let skip = ResourceState::Invalid { reason: "target is a directory".into() };
///
/// assert_ne!(missing, correct);
/// assert_eq!(correct, ResourceState::Correct);
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
}

impl std::fmt::Display for ResourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "missing"),
            Self::Correct => write!(f, "correct"),
            Self::Incorrect { current } => write!(f, "incorrect (current: {current})"),
            Self::Invalid { reason } => write!(f, "invalid ({reason})"),
        }
    }
}

/// Result of applying a resource change.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::resources::ResourceChange;
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
    /// Resource was skipped (e.g., missing source file, or target is a protected directory).
    Skipped {
        /// Reason why the resource was skipped.
        reason: String,
    },
}

/// Unified interface for resources that can be checked and applied.
///
/// Extends [`Applicable`] with state-checking methods for resources that can
/// independently determine their own state (e.g. symlinks, registry entries,
/// file permissions).
///
/// Resources whose state requires a single external bulk query should implement
/// only [`Applicable`] instead.
///
/// # Examples
///
/// ```ignore
/// // All resources follow the same check-then-apply pattern:
/// let state = resource.current_state()?;
/// if resource.needs_change()? {
///     resource.apply()?;
/// }
/// ```
pub trait Resource: Applicable {
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
    #[allow(dead_code)] // Part of trait contract; used in tests
    fn needs_change(&self) -> Result<bool> {
        Ok(matches!(
            self.current_state()?,
            ResourceState::Missing | ResourceState::Incorrect { .. }
        ))
    }
}

/// Shared test helpers for resource unit tests.
///
/// Re-exports [`TestExecutor`](crate::exec::test_helpers::TestExecutor) as
/// `MockExecutor` for backward compatibility with existing tests.
#[cfg(test)]
pub mod test_helpers {
    /// Backward-compatible alias for [`TestExecutor`](crate::exec::test_helpers::TestExecutor).
    pub use crate::exec::test_helpers::TestExecutor as MockExecutor;
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    struct TestResource {
        state: ResourceState,
    }

    impl Applicable for TestResource {
        fn description(&self) -> String {
            "test resource".to_string()
        }

        fn apply(&self) -> Result<ResourceChange> {
            Ok(ResourceChange::Applied)
        }
    }

    impl Resource for TestResource {
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
