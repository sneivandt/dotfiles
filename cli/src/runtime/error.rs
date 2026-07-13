//! Generic runtime and configuration error types.
//!
//! This module provides a structured error type using [`thiserror`].
//! Internal modules return typed errors (e.g., [`ConfigError`])
//! while command handlers at the CLI boundary convert them to [`anyhow::Error`]
//! via the standard `?` operator.

use thiserror::Error;

/// Aggregate failure reported after one or more tasks already logged their
/// individual errors.
#[derive(Error, Debug)]
#[error("{count} task(s) failed")]
pub(crate) struct TaskFailures {
    count: usize,
}

impl TaskFailures {
    /// Create an aggregate task failure for the completed run.
    pub(crate) const fn new(count: usize) -> Self {
        Self { count }
    }
}

/// Errors that arise from configuration loading and profile resolution.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// The requested profile name is not defined in `conf/profiles.toml`.
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
    #[error("I/O error reading config file {path}: {source}")]
    Io {
        /// Path to the file that could not be read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code uses panicking helpers")]
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
        assert!(e.to_string().contains("I/O error reading config file"));
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
}
