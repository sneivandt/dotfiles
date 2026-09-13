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

/// Errors that arise from profile resolution.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// The requested profile name is not built into the CLI.
    #[error("Invalid profile '{name}' (available: {available})")]
    InvalidProfile {
        /// The profile name that was requested.
        name: String,
        /// Comma-separated list of valid profile names.
        available: String,
    },
}

#[cfg(test)]
mod tests {
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
