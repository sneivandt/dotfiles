//! Typed error variants for resource operations.
//!
//! This module provides [`ResourceError`], a structured error type for
//! resource check and apply operations.  Internal resource code may return
//! these variants directly; callers convert to [`anyhow::Error`] via `?`.

use thiserror::Error;

/// Errors that arise from resource checks and apply operations.
#[derive(Error, Debug)]
pub enum ResourceError {
    /// A command invoked by a resource failed with a non-zero exit code.
    #[error("command '{program}' failed (exit {exit_code}): {stderr}")]
    ExecutionFailed {
        /// Name of the program that was invoked.
        program: String,
        /// Exit code returned by the process.
        exit_code: i32,
        /// Captured standard error output.
        stderr: String,
    },

    /// A required resource (file, package, etc.) was not found.
    #[error("resource not found: {resource}")]
    NotFound {
        /// Description of the missing resource.
        resource: String,
    },

    /// An operation was denied due to insufficient permissions.
    #[error("permission denied: {path}")]
    PermissionDenied {
        /// Path or resource for which permission was denied.
        path: String,
    },

    /// A resource exists but is in an unexpected or inconsistent state.
    #[error("invalid state for '{resource}': {reason}")]
    InvalidState {
        /// Name or description of the resource in the invalid state.
        resource: String,
        /// Human-readable explanation of why the state is invalid.
        reason: String,
    },

    /// The requested operation is not supported for this resource type.
    #[error("operation '{operation}' is not supported for resource '{resource}'")]
    UnsupportedOperation {
        /// Name of the unsupported operation (e.g. `"remove"`).
        operation: String,
        /// Name or description of the resource.
        resource: String,
    },
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn execution_failed_display() {
        let e = ResourceError::ExecutionFailed {
            program: "pacman".to_string(),
            exit_code: 1,
            stderr: "target not found".to_string(),
        };
        assert!(e.to_string().contains("pacman"));
        assert!(e.to_string().contains("exit 1"));
        assert!(e.to_string().contains("target not found"));
    }

    #[test]
    fn not_found_display() {
        let e = ResourceError::NotFound {
            resource: "git".to_string(),
        };
        assert_eq!(e.to_string(), "resource not found: git");
    }

    #[test]
    fn permission_denied_display() {
        let e = ResourceError::PermissionDenied {
            path: "/etc/hosts".to_string(),
        };
        assert!(e.to_string().contains("/etc/hosts"));
    }

    #[test]
    fn invalid_state_display() {
        let e = ResourceError::InvalidState {
            resource: "~/.bashrc".to_string(),
            reason: "target is a directory".to_string(),
        };
        assert!(e.to_string().contains("~/.bashrc"));
        assert!(e.to_string().contains("target is a directory"));
    }

    #[test]
    fn unsupported_operation_display() {
        let e = ResourceError::UnsupportedOperation {
            operation: "remove".to_string(),
            resource: "git".to_string(),
        };
        assert!(e.to_string().contains("remove"));
        assert!(e.to_string().contains("git"));
    }

    #[test]
    fn resource_error_converts_to_anyhow() {
        let e = ResourceError::NotFound {
            resource: "vim".to_string(),
        };
        let _anyhow_err: anyhow::Error = e.into();
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn resource_error_is_send_sync() {
        assert_send_sync::<ResourceError>();
    }
}
