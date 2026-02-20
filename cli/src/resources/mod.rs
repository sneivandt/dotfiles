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
///
/// # Examples
///
/// All resources follow the same pattern:
/// 1. Check current state: `resource.current_state()?`
/// 2. Apply if needed: `resource.apply()?`
/// 3. Remove if supported: `resource.remove()?`
pub trait Resource {
    /// Human-readable description of this resource.
    fn description(&self) -> String;

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

    /// Apply the resource change.
    ///
    /// This method should:
    /// - Create parent directories if needed
    /// - Update the resource to match the desired state
    /// - Return the appropriate `ResourceChange` result
    ///
    /// This method is only called when NOT in dry-run mode and when
    /// `needs_change()` returns `true`.
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
        anyhow::bail!("remove not supported for {}", self.description())
    }
}

/// Shared test helpers for resource unit tests.
///
/// Provides a configurable [`MockExecutor`] so individual resource test
/// modules do not have to duplicate the boilerplate.
#[cfg(test)]
pub(crate) mod test_helpers {
    use crate::exec::{ExecResult, Executor};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Mutex;

    /// A configurable mock executor for resource unit tests.
    ///
    /// Maintains a queue of `(success, stdout)` responses consumed in FIFO
    /// order.  When the queue is empty any call returns a failed response
    /// (`success = false`, stdout = `"unexpected call"`).
    #[derive(Debug)]
    pub struct MockExecutor {
        responses: Mutex<VecDeque<(bool, String)>>,
    }

    impl MockExecutor {
        /// Create a mock with a single successful response.
        pub fn ok(stdout: &str) -> Self {
            Self::with_responses(vec![(true, stdout.to_string())])
        }

        /// Create a mock with a single failed response (empty stdout).
        pub fn fail() -> Self {
            Self::with_responses(vec![(false, String::new())])
        }

        /// Create a mock with an explicit success flag and stdout value.
        pub fn with_output(success: bool, stdout: &str) -> Self {
            Self::with_responses(vec![(success, stdout.to_string())])
        }

        /// Create a mock from an ordered list of `(success, stdout)` pairs.
        pub fn with_responses(responses: Vec<(bool, String)>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
            }
        }

        fn next(&self) -> (bool, String) {
            self.responses.lock().map_or_else(
                |_| (false, "mutex poisoned".to_string()),
                |mut guard| {
                    guard
                        .pop_front()
                        .unwrap_or_else(|| (false, "unexpected call".to_string()))
                },
            )
        }
    }

    impl Executor for MockExecutor {
        fn run(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_in(&self, _: &Path, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<ExecResult> {
            let (success, stdout) = self.next();
            if success {
                Ok(ExecResult {
                    stdout,
                    stderr: String::new(),
                    success: true,
                    code: Some(0),
                })
            } else {
                anyhow::bail!("mock command failed")
            }
        }

        fn run_unchecked(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            let (success, stdout) = self.next();
            Ok(ExecResult {
                stdout,
                stderr: String::new(),
                success,
                code: Some(i32::from(!success)),
            })
        }

        fn which(&self, _: &str) -> bool {
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
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
