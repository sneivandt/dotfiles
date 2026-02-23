//! Domain-specific error types for the dotfiles engine.
//!
//! This module provides a structured error type using [`thiserror`].
//! Internal modules return typed errors (e.g., [`ConfigError`])
//! while command handlers at the CLI boundary convert them to [`anyhow::Error`]
//! via the standard `?` operator.

use thiserror::Error;

/// Errors that arise from configuration loading and profile resolution.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// The requested profile name is not defined in `profiles.ini`.
    ///
    /// Valid profiles are defined in [`crate::config::profiles::PROFILE_NAMES`].
    #[error("Invalid profile '{0}'")]
    InvalidProfile(String),

    /// A required INI section is absent from the config file.
    #[error("Missing required section [{0}]")]
    MissingSection(String),

    /// The INI file contains a syntax error that prevents parsing.
    #[error("Invalid INI syntax in {file}: {message}")]
    InvalidSyntax { file: String, message: String },

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
        let e = ConfigError::InvalidProfile("unknown".to_string());
        assert_eq!(e.to_string(), "Invalid profile 'unknown'");
    }

    #[test]
    fn config_error_missing_section_display() {
        let e = ConfigError::MissingSection("packages".to_string());
        assert_eq!(e.to_string(), "Missing required section [packages]");
    }

    #[test]
    fn config_error_invalid_syntax_display() {
        let e = ConfigError::InvalidSyntax {
            file: "packages.ini".to_string(),
            message: "unexpected token".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "Invalid INI syntax in packages.ini: unexpected token"
        );
    }

    #[test]
    fn config_error_io_display() {
        let e = ConfigError::Io {
            path: "/conf/packages.ini".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "no such file"),
        };
        assert!(e.to_string().contains("/conf/packages.ini"));
        assert!(e.to_string().contains("IO error reading config file"));
    }

    #[test]
    fn config_error_io_has_source() {
        use std::error::Error as StdError;
        let e = ConfigError::Io {
            path: "/conf/packages.ini".to_string(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
        };
        assert!(e.source().is_some());
    }

    // -----------------------------------------------------------------------
    // anyhow conversion
    // -----------------------------------------------------------------------

    #[test]
    fn config_error_converts_to_anyhow() {
        let e = ConfigError::InvalidProfile("bad".to_string());
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
