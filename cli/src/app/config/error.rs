//! Errors raised while loading and resolving application configuration.

use thiserror::Error;

use crate::infra::config::Diagnostic;

/// Only desired-state conflicts are fatal here; ordinary validation findings
/// are reported after loading without preventing snapshot publication.
pub(super) fn reject_conflicts(
    conflicts: impl IntoIterator<Item = Diagnostic>,
) -> anyhow::Result<()> {
    let messages: Vec<_> = conflicts
        .into_iter()
        .map(|diagnostic| {
            format!(
                "  {} [{}] ({}): {}",
                diagnostic.source, diagnostic.item, diagnostic.code, diagnostic.message
            )
        })
        .collect();
    anyhow::ensure!(
        messages.is_empty(),
        "contradictory desired state:\n{}",
        messages.join("\n")
    );
    Ok(())
}

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
    fn io_preserves_source_and_path() {
        use std::error::Error as _;

        let error = ConfigError::Io {
            path: "/conf/packages.toml".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "no such file"),
        };
        assert_eq!(
            error.to_string(),
            "I/O error reading config file /conf/packages.toml: no such file"
        );
        assert_eq!(error.source().unwrap().to_string(), "no such file");
    }

    #[test]
    fn conflicts_preserve_header_and_diagnostic_order() {
        use crate::infra::config::DiagnosticCode;

        reject_conflicts([]).unwrap();
        let error = reject_conflicts([
            Diagnostic::error(
                "main.toml",
                "first",
                DiagnosticCode::new("test", "first"),
                "first conflict",
            ),
            Diagnostic::error(
                "overlay.toml",
                "second",
                DiagnosticCode::new("test", "second"),
                "second conflict",
            ),
        ])
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "contradictory desired state:\n  main.toml [first] (test.first): first conflict\n  overlay.toml [second] (test.second): second conflict"
        );
    }
}
