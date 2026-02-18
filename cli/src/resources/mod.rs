pub mod chmod;
pub mod copilot_skill;
pub mod package;
pub mod registry;
pub mod symlink;
pub mod systemd_unit;
pub mod vscode_extension;

use anyhow::Result;

/// State of a resource (file, registry entry, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceState {
    /// Resource does not exist or is not present.
    Missing,
    /// Resource exists and matches the desired state.
    Correct,
    /// Resource exists but does not match the desired state.
    Incorrect { current: String },
    /// Resource cannot be applied (e.g., target is a directory that shouldn't be removed).
    Invalid { reason: String },
}

/// Result of applying a resource change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Some variants used in tests or future implementations
pub enum ResourceChange {
    /// Resource was created or updated.
    Applied,
    /// Resource was already correct (no change needed).
    AlreadyCorrect,
    /// Resource was skipped (e.g., missing source file, or target is a protected directory).
    Skipped { reason: String },
}

/// Unified interface for resources that can be checked and applied.
///
/// This abstraction provides consistent handling for:
/// - Symlinks (file system links)
/// - Registry entries (Windows registry)
/// - File permissions (chmod on Unix)
/// - Other declarative resources
pub trait Resource {
    /// Human-readable description of this resource.
    fn description(&self) -> String;

    /// Check the current state of the resource.
    fn current_state(&self) -> Result<ResourceState>;

    /// Determine if the resource needs to be changed.
    #[allow(dead_code)] // Used in tests and may be useful in future
    fn needs_change(&self) -> Result<bool> {
        Ok(matches!(
            self.current_state()?,
            ResourceState::Missing | ResourceState::Incorrect { .. }
        ))
    }

    /// Apply the resource change.
    ///
    /// This method should:
    /// - Create parent directories if needed
    /// - Update the resource to match the desired state
    /// - Return the appropriate `ResourceChange` result
    ///
    /// This method is only called when NOT in dry-run mode and when
    /// `needs_change()` returns `true`.
    fn apply(&self) -> Result<ResourceChange>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestResource {
        state: ResourceState,
    }

    impl Resource for TestResource {
        fn description(&self) -> String {
            "test resource".to_string()
        }

        fn current_state(&self) -> Result<ResourceState> {
            Ok(self.state.clone())
        }

        fn apply(&self) -> Result<ResourceChange> {
            Ok(ResourceChange::Applied)
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
}
