//! Errors raised while loading and resolving application configuration.

use thiserror::Error;

/// Errors that arise from configuration loading and profile resolution.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// The requested profile name is not defined in `conf/profiles.toml`.
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
    use std::io;

    use super::*;

    #[test]
    fn invalid_profile_display() {
        let error = ConfigError::InvalidProfile {
            name: "unknown".to_string(),
            available: "base, desktop".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Invalid profile 'unknown' (available: base, desktop)"
        );
    }

    #[test]
    fn io_display_includes_path() {
        let error = ConfigError::Io {
            path: "/conf/packages.toml".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "no such file"),
        };
        assert!(error.to_string().contains("/conf/packages.toml"));
        assert!(error.to_string().contains("I/O error reading config file"));
    }

    #[test]
    fn io_preserves_source() {
        use std::error::Error as _;

        let error = ConfigError::Io {
            path: "/conf/packages.toml".to_string(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
        };
        assert!(error.source().is_some());
    }

    #[test]
    fn converts_to_anyhow() {
        let error = ConfigError::InvalidProfile {
            name: "bad".to_string(),
            available: "base, desktop".to_string(),
        };
        let _anyhow_error: anyhow::Error = error.into();
    }

    #[test]
    fn is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<ConfigError>();
    }
}
