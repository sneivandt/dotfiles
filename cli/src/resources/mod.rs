//! Idempotent resource primitives (check + apply pattern).
pub mod chmod;
pub mod copilot_skill;
pub mod developer_mode;
pub mod fs;
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
        anyhow::bail!(
            "operation 'remove' is not supported for resource '{}'",
            self.description()
        )
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
/// Provides a configurable [`MockExecutor`] so individual resource test
/// modules do not have to duplicate the boilerplate.
#[cfg(test)]
pub mod test_helpers {
    use crate::exec::{ExecResult, Executor};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    /// A configurable mock executor for resource unit tests.
    ///
    /// Maintains a queue of `(success, stdout)` responses consumed in FIFO
    /// order.  When the queue is empty any call returns a failed response
    /// (`success = false`, stdout = `"unexpected call"`).
    ///
    /// Use [`with_which`](Self::with_which) to configure the value returned
    /// by [`Executor::which`] (defaults to `false`).
    ///
    /// Use [`call_count`](Self::call_count) to inspect how many executor calls
    /// were made.
    #[derive(Debug)]
    pub struct MockExecutor {
        responses: Mutex<VecDeque<(bool, String)>>,
        /// Return value for every [`Executor::which`] call.
        which_result: bool,
        call_count: Arc<AtomicUsize>,
    }

    impl MockExecutor {
        /// Create a mock with a single successful response.
        #[must_use]
        pub fn ok(stdout: &str) -> Self {
            Self::with_responses(vec![(true, stdout.to_string())])
        }

        /// Create a mock with a single failed response (empty stdout).
        #[must_use]
        pub fn fail() -> Self {
            Self::with_responses(vec![(false, String::new())])
        }

        /// Create a mock from an ordered list of `(success, stdout)` pairs.
        #[must_use]
        pub fn with_responses(responses: Vec<(bool, String)>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                which_result: false,
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }

        /// Set the value returned by every [`Executor::which`] call.
        #[must_use]
        pub fn with_which(mut self, result: bool) -> Self {
            self.which_result = result;
            self
        }

        /// Return the total number of executor calls made so far.
        #[must_use]
        pub fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }

        fn next(&self) -> (bool, String) {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.responses.lock().map_or_else(
                |_| (false, "mutex poisoned".to_string()),
                |mut guard| {
                    guard
                        .pop_front()
                        .unwrap_or_else(|| (false, "unexpected call".to_string()))
                },
            )
        }

        fn next_result(&self) -> anyhow::Result<ExecResult> {
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
    }

    impl Executor for MockExecutor {
        fn run(&self, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            self.next_result()
        }

        fn run_in(&self, _: &Path, _: &str, _: &[&str]) -> anyhow::Result<ExecResult> {
            self.next_result()
        }

        fn run_in_with_env(
            &self,
            _: &Path,
            _: &str,
            _: &[&str],
            _: &[(&str, &str)],
        ) -> anyhow::Result<ExecResult> {
            self.next_result()
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
            self.which_result
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
