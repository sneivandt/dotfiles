//! Errors produced by generic resource operations.

use thiserror::Error;

/// Errors that arise from resource operations (check, apply, remove).
///
/// Resources return a typed `ResourceError` so the engine can classify
/// failures while preserving context-rich helper errors through [`Other`](Self::Other).
///
/// # Examples
///
/// ```
/// use dotfiles_cli::testing::error::ResourceError;
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
    #[allow(
        dead_code,
        reason = "part of the resource error taxonomy; currently constructed only by Unix-only resources"
    )]
    PermissionDenied {
        /// Path that could not be accessed.
        path: String,
    },

    /// The resource was found in an unexpected state that cannot be reconciled.
    #[error("conflicting state for {resource}: expected {expected}, found {actual}")]
    #[allow(
        dead_code,
        reason = "part of the resource error taxonomy; exercised through test resources"
    )]
    ConflictingState {
        /// Human-readable resource description.
        resource: String,
        /// Expected or desired state.
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

    /// An I/O error occurred while applying or removing a resource.
    #[error("I/O error: {source}")]
    Io {
        /// The underlying I/O error.
        #[from]
        source: std::io::Error,
    },

    /// A resource failure that does not map to a more specific variant.
    #[error(transparent)]
    Other {
        /// The underlying error, preserving its context chain.
        #[from]
        source: anyhow::Error,
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
    #[allow(
        dead_code,
        reason = "part of the resource error taxonomy; currently used only by Unix-only resources"
    )]
    pub fn permission_denied(path: impl Into<String>) -> Self {
        Self::PermissionDenied { path: path.into() }
    }

    /// Create a [`ConflictingState`](Self::ConflictingState) error.
    #[must_use]
    #[allow(
        dead_code,
        reason = "part of the resource error taxonomy; exercised through test resources"
    )]
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

    /// Short, stable category label for diagnostic logging.
    ///
    /// Recurses into [`Other`](Self::Other) so a typed error converted through
    /// [`anyhow::Error`] retains its original category.
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            Self::CommandFailed { .. } => "command_failed",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::ConflictingState { .. } => "conflicting_state",
            Self::NotSupported { .. } => "not_supported",
            Self::Io { .. } => "io",
            Self::Other { source } => source
                .downcast_ref::<Self>()
                .map_or("unknown", Self::category),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "test code uses panicking helpers")]
mod tests {
    use super::*;
    use std::io;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn resource_error_command_failed_display() {
        let error = ResourceError::command_failed("pacman", "exit code 1");
        assert!(error.to_string().contains("pacman"));
        assert!(error.to_string().contains("exit code 1"));
    }

    #[test]
    fn resource_error_permission_denied_display() {
        let error = ResourceError::permission_denied("/etc/passwd");
        assert!(error.to_string().contains("/etc/passwd"));
        assert!(error.to_string().contains("permission denied"));
    }

    #[test]
    fn resource_error_conflicting_state_display() {
        let error = ResourceError::conflicting_state("~/.bashrc", "symlink", "regular file");
        let message = error.to_string();
        assert!(message.contains("~/.bashrc"));
        assert!(message.contains("symlink"));
        assert!(message.contains("regular file"));
    }

    #[test]
    fn resource_error_not_supported_display() {
        let error = ResourceError::not_supported("systemd not available");
        assert!(error.to_string().contains("systemd not available"));
    }

    #[test]
    fn resource_error_converts_to_anyhow() {
        let error = ResourceError::command_failed("git", "not found");
        let _anyhow_error: anyhow::Error = error.into();
    }

    #[test]
    fn resource_error_is_send_sync() {
        assert_send_sync::<ResourceError>();
    }

    #[test]
    fn category_labels_match_variants() {
        assert_eq!(
            ResourceError::command_failed("git", "x").category(),
            "command_failed"
        );
        assert_eq!(
            ResourceError::permission_denied("/p").category(),
            "permission_denied"
        );
        assert_eq!(
            ResourceError::conflicting_state("r", "e", "a").category(),
            "conflicting_state"
        );
        assert_eq!(
            ResourceError::not_supported("r").category(),
            "not_supported"
        );
        let io_error: ResourceError = io::Error::new(io::ErrorKind::NotFound, "nope").into();
        assert_eq!(io_error.category(), "io");
        let other: ResourceError = anyhow::anyhow!("freeform").into();
        assert_eq!(other.category(), "unknown");
    }

    #[test]
    fn category_recurses_through_anyhow_round_trip() {
        let typed = ResourceError::command_failed("pacman", "exit 1");
        let as_anyhow: anyhow::Error = typed.into();
        let round_tripped: ResourceError = as_anyhow.into();
        assert!(matches!(round_tripped, ResourceError::Other { .. }));
        assert_eq!(round_tripped.category(), "command_failed");
    }

    #[test]
    fn other_variant_preserves_context_chain() {
        let chained = anyhow::anyhow!("leaf cause").context("outer context");
        let error: ResourceError = chained.into();
        let as_anyhow: anyhow::Error = error.into();
        let rendered = format!("{as_anyhow:#}");
        assert!(rendered.contains("outer context"), "got: {rendered}");
        assert!(rendered.contains("leaf cause"), "got: {rendered}");
    }
}
