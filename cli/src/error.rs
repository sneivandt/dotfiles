//! Domain-specific error types for the dotfiles engine.
//!
//! This module provides a structured error type using [`thiserror`].
//! Internal modules return typed errors (e.g., [`ConfigError`])
//! while command handlers at the CLI boundary convert them to [`anyhow::Error`]
//! via the standard `?` operator.

use thiserror::Error;

/// Errors that arise from resource operations (check, apply, remove).
///
/// Resources return `anyhow::Result` for flexibility, but can wrap a
/// `ResourceError` variant to enable pattern-matched recovery or
/// categorised failure reporting in the processing layer.
///
/// Use the factory methods ([`command_failed`](Self::command_failed),
/// [`not_supported`](Self::not_supported), etc.) for concise construction.
///
/// # Examples
///
/// ```
/// use dotfiles_cli::error::ResourceError;
///
/// let err = ResourceError::command_failed("pacman", "exit code 1");
/// assert!(err.to_string().contains("pacman"));
/// ```
#[derive(Error, Debug)]
pub enum ResourceError {
    /// An external command required by the resource failed.
    #[error("command '{program}' failed: {message}")]
    CommandFailed {
        /// The program that was invoked.
        program: String,
        /// Human-readable failure description (typically stderr or exit info).
        message: String,
    },

    /// The operation was denied due to insufficient permissions.
    #[error("permission denied: {path}")]
    PermissionDenied {
        /// Path that could not be accessed.
        path: String,
    },

    /// The resource was found in an unexpected state that conflicts with the
    /// desired state and cannot be automatically reconciled.
    #[error("conflicting state for {resource}: expected {expected}, found {actual}")]
    ConflictingState {
        /// Human-readable resource description.
        resource: String,
        /// Expected / desired state.
        expected: String,
        /// Actual state found on disk.
        actual: String,
    },

    /// The resource operation is not supported on the current platform.
    #[error("resource not supported on this platform: {reason}")]
    NotSupported {
        /// Explanation of why the resource is unsupported.
        reason: String,
    },
}

impl ResourceError {
    /// Create a [`CommandFailed`](Self::CommandFailed) error.
    #[must_use]
    pub fn command_failed(program: impl Into<String>, message: impl Into<String>) -> Self {
        Self::CommandFailed {
            program: program.into(),
            message: message.into(),
        }
    }

    /// Create a [`PermissionDenied`](Self::PermissionDenied) error.
    #[must_use]
    pub fn permission_denied(path: impl Into<String>) -> Self {
        Self::PermissionDenied { path: path.into() }
    }

    /// Create a [`ConflictingState`](Self::ConflictingState) error.
    #[must_use]
    pub fn conflicting_state(
        resource: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::ConflictingState {
            resource: resource.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create a [`NotSupported`](Self::NotSupported) error.
    #[must_use]
    pub fn not_supported(reason: impl Into<String>) -> Self {
        Self::NotSupported {
            reason: reason.into(),
        }
    }
}

/// Errors that arise from configuration loading and profile resolution.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// The requested profile name is not defined in `profiles.toml`.
    ///
    /// Valid profiles are defined in `conf/profiles.toml`.
    #[error("Invalid profile '{name}' (available: {available})")]
    InvalidProfile {
        /// The profile name that was requested.
        name: String,
        /// Comma-separated list of valid profile names.
        available: String,
    },

    /// The TOML file contains a syntax error that prevents parsing.
    #[error("Invalid TOML syntax in {path}: {source}")]
    TomlParse {
        /// Path to the file that could not be parsed.
        path: String,
        /// Underlying TOML parse error.
        source: toml::de::Error,
    },

    /// An I/O error occurred while reading a config file.
    #[error("IO error reading config file {path}: {source}")]
    Io {
        /// Path to the file that could not be read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use std::io;

    // -----------------------------------------------------------------------
    // ConfigError
    // -----------------------------------------------------------------------

    #[test]
    fn config_error_invalid_profile_display() {
        let e = ConfigError::InvalidProfile {
            name: "unknown".to_string(),
            available: "base, desktop".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "Invalid profile 'unknown' (available: base, desktop)"
        );
    }

    #[test]
    fn config_error_io_display() {
        let e = ConfigError::Io {
            path: "/conf/packages.toml".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "no such file"),
        };
        assert!(e.to_string().contains("/conf/packages.toml"));
        assert!(e.to_string().contains("IO error reading config file"));
    }

    #[test]
    fn config_error_io_has_source() {
        use std::error::Error as StdError;
        let e = ConfigError::Io {
            path: "/conf/packages.toml".to_string(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
        };
        assert!(e.source().is_some());
    }

    // -----------------------------------------------------------------------
    // anyhow conversion
    // -----------------------------------------------------------------------

    #[test]
    fn config_error_converts_to_anyhow() {
        let e = ConfigError::InvalidProfile {
            name: "bad".to_string(),
            available: "base, desktop".to_string(),
        };
        let _anyhow_err: anyhow::Error = e.into();
    }

    // -----------------------------------------------------------------------
    // Send + Sync bounds
    // -----------------------------------------------------------------------

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn config_error_is_send_sync() {
        assert_send_sync::<ConfigError>();
    }

    // -----------------------------------------------------------------------
    // ResourceError
    // -----------------------------------------------------------------------

    #[test]
    fn resource_error_command_failed_display() {
        let e = ResourceError::command_failed("pacman", "exit code 1");
        assert!(e.to_string().contains("pacman"));
        assert!(e.to_string().contains("exit code 1"));
    }

    #[test]
    fn resource_error_permission_denied_display() {
        let e = ResourceError::permission_denied("/etc/passwd");
        assert!(e.to_string().contains("/etc/passwd"));
        assert!(e.to_string().contains("permission denied"));
    }

    #[test]
    fn resource_error_conflicting_state_display() {
        let e = ResourceError::conflicting_state("~/.bashrc", "symlink", "regular file");
        let msg = e.to_string();
        assert!(msg.contains("~/.bashrc"));
        assert!(msg.contains("symlink"));
        assert!(msg.contains("regular file"));
    }

    #[test]
    fn resource_error_not_supported_display() {
        let e = ResourceError::not_supported("systemd not available");
        assert!(e.to_string().contains("systemd not available"));
    }

    #[test]
    fn resource_error_converts_to_anyhow() {
        let e = ResourceError::command_failed("git", "not found");
        let _anyhow_err: anyhow::Error = e.into();
    }

    #[test]
    fn resource_error_is_send_sync() {
        assert_send_sync::<ResourceError>();
    }

    #[test]
    fn example_failing_test() {
        assert_eq!(1, 2, "This test intentionally fails to trigger the CI fix agent");
    }
}
