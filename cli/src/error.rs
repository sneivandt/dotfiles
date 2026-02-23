//! Domain-specific error types for the dotfiles engine.
//!
//! This module provides a structured error hierarchy using [`thiserror`].
//! Internal modules return typed errors (e.g., [`ConfigError`], [`TaskError`])
//! while command handlers at the CLI boundary convert them to [`anyhow::Error`]
//! via the standard `?` operator.
//!
//! # Error hierarchy
//!
//! ```text
//! DotfilesError
//! ├── Config(ConfigError)    — INI parsing, profile resolution
//! ├── Task(TaskError)        — task execution and dependency issues
//! ├── Resource(ResourceError)— symlinks, packages, permissions
//! └── Platform(PlatformError)— OS-specific operation failures
//! ```

// Error types are part of the public API and are being introduced for gradual
// migration. Not all variants are used in existing code yet.
#![allow(dead_code)]

use thiserror::Error;

/// Top-level error type for the dotfiles engine.
///
/// Aggregates domain-specific sub-errors and is convertible to
/// [`anyhow::Error`] for use at CLI command boundaries.
#[derive(Error, Debug)]
pub enum DotfilesError {
    /// Configuration-related error (parsing, profile resolution, I/O).
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// Task execution error (failure, missing dependency, dependency cycle).
    #[error("Task execution error: {0}")]
    Task(#[from] TaskError),

    /// Resource operation error (symlink, package install, permissions).
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),

    /// Platform-specific operation error (unsupported operation, detection failure).
    #[error("Platform error: {0}")]
    Platform(#[from] PlatformError),
}

/// Errors that arise from configuration loading and profile resolution.
#[derive(Error, Debug)]
pub enum ConfigError {
    /// The requested profile name is not defined in `profiles.ini`.
    #[error("Invalid profile '{0}': must be one of base, desktop")]
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

/// Errors that arise during task execution.
#[derive(Error, Debug)]
pub enum TaskError {
    /// A task failed to execute.
    #[error("Task '{task}' failed: {reason}")]
    ExecutionFailed {
        /// Name of the task that failed.
        task: String,
        /// Human-readable reason for the failure.
        reason: String,
    },

    /// The task dependency graph contains a cycle.
    #[error("Task dependency cycle detected: {0}")]
    DependencyCycle(String),

    /// A required dependency is not present.
    #[error("Required dependency '{0}' not found")]
    MissingDependency(String),
}

/// Errors that arise from resource operations (symlinks, packages, permissions).
#[derive(Error, Debug)]
pub enum ResourceError {
    /// A symlink operation failed.
    #[error("Symlink error: {0}")]
    Symlink(String),

    /// A package installation failed.
    #[error("Package installation failed: {package}")]
    PackageInstall {
        /// Name of the package that could not be installed.
        package: String,
        /// Underlying error from the package manager.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A file permission change failed.
    #[error("File permission error: {path}")]
    Permission {
        /// Path of the file whose permissions could not be changed.
        path: String,
    },

    /// A required file was not found.
    #[error("File not found: {0}")]
    NotFound(String),
}

/// Errors that arise from platform-specific operations.
#[derive(Error, Debug)]
pub enum PlatformError {
    /// The requested operation is not supported on the current platform.
    #[error("Operation not supported on {platform}")]
    Unsupported {
        /// Name of the platform (e.g., `"Windows"`, `"Linux"`).
        platform: String,
    },

    /// Platform detection failed (e.g., unknown OS or missing system info).
    #[error("Platform detection failed: {0}")]
    DetectionFailed(String),
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
        assert_eq!(
            e.to_string(),
            "Invalid profile 'unknown': must be one of base, desktop"
        );
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
    // TaskError
    // -----------------------------------------------------------------------

    #[test]
    fn task_error_execution_failed_display() {
        let e = TaskError::ExecutionFailed {
            task: "InstallPackages".to_string(),
            reason: "pacman exited with code 1".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "Task 'InstallPackages' failed: pacman exited with code 1"
        );
    }

    #[test]
    fn task_error_dependency_cycle_display() {
        let e = TaskError::DependencyCycle("A → B → A".to_string());
        assert_eq!(e.to_string(), "Task dependency cycle detected: A → B → A");
    }

    #[test]
    fn task_error_missing_dependency_display() {
        let e = TaskError::MissingDependency("UpdateRepository".to_string());
        assert_eq!(
            e.to_string(),
            "Required dependency 'UpdateRepository' not found"
        );
    }

    // -----------------------------------------------------------------------
    // ResourceError
    // -----------------------------------------------------------------------

    #[test]
    fn resource_error_symlink_display() {
        let e = ResourceError::Symlink("failed to create ~/.bashrc".to_string());
        assert_eq!(e.to_string(), "Symlink error: failed to create ~/.bashrc");
    }

    #[test]
    fn resource_error_package_install_display() {
        let e = ResourceError::PackageInstall {
            package: "neovim".to_string(),
            source: "pacman: package not found".into(),
        };
        assert_eq!(e.to_string(), "Package installation failed: neovim");
    }

    #[test]
    fn resource_error_package_install_has_source() {
        use std::error::Error as StdError;
        let e = ResourceError::PackageInstall {
            package: "neovim".to_string(),
            source: "pacman: package not found".into(),
        };
        assert!(e.source().is_some());
    }

    #[test]
    fn resource_error_permission_display() {
        let e = ResourceError::Permission {
            path: "~/.ssh/id_rsa".to_string(),
        };
        assert_eq!(e.to_string(), "File permission error: ~/.ssh/id_rsa");
    }

    #[test]
    fn resource_error_not_found_display() {
        let e = ResourceError::NotFound("~/.config/nvim/init.lua".to_string());
        assert_eq!(e.to_string(), "File not found: ~/.config/nvim/init.lua");
    }

    // -----------------------------------------------------------------------
    // PlatformError
    // -----------------------------------------------------------------------

    #[test]
    fn platform_error_unsupported_display() {
        let e = PlatformError::Unsupported {
            platform: "Windows".to_string(),
        };
        assert_eq!(e.to_string(), "Operation not supported on Windows");
    }

    #[test]
    fn platform_error_detection_failed_display() {
        let e = PlatformError::DetectionFailed("unknown OS identifier".to_string());
        assert_eq!(
            e.to_string(),
            "Platform detection failed: unknown OS identifier"
        );
    }

    // -----------------------------------------------------------------------
    // DotfilesError conversions
    // -----------------------------------------------------------------------

    #[test]
    fn dotfiles_error_from_config_error() {
        let config_err = ConfigError::InvalidProfile("bad".to_string());
        let e: DotfilesError = config_err.into();
        assert!(e.to_string().contains("Configuration error"));
        assert!(e.to_string().contains("bad"));
    }

    #[test]
    fn dotfiles_error_from_task_error() {
        let task_err = TaskError::DependencyCycle("A → A".to_string());
        let e: DotfilesError = task_err.into();
        assert!(e.to_string().contains("Task execution error"));
    }

    #[test]
    fn dotfiles_error_from_resource_error() {
        let res_err = ResourceError::NotFound("file.txt".to_string());
        let e: DotfilesError = res_err.into();
        assert!(e.to_string().contains("Resource error"));
    }

    #[test]
    fn dotfiles_error_from_platform_error() {
        let plat_err = PlatformError::DetectionFailed("no info".to_string());
        let e: DotfilesError = plat_err.into();
        assert!(e.to_string().contains("Platform error"));
    }

    // -----------------------------------------------------------------------
    // Send + Sync bounds
    // -----------------------------------------------------------------------

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn all_error_types_are_send_sync() {
        assert_send_sync::<DotfilesError>();
        assert_send_sync::<ConfigError>();
        assert_send_sync::<TaskError>();
        assert_send_sync::<ResourceError>();
        assert_send_sync::<PlatformError>();
    }

    // -----------------------------------------------------------------------
    // anyhow conversion
    // -----------------------------------------------------------------------

    #[test]
    fn config_error_converts_to_anyhow() {
        let e = ConfigError::InvalidProfile("bad".to_string());
        let _anyhow_err: anyhow::Error = e.into();
    }

    #[test]
    fn task_error_converts_to_anyhow() {
        let e = TaskError::MissingDependency("X".to_string());
        let _anyhow_err: anyhow::Error = e.into();
    }

    #[test]
    fn resource_error_converts_to_anyhow() {
        let e = ResourceError::Symlink("oops".to_string());
        let _anyhow_err: anyhow::Error = e.into();
    }

    #[test]
    fn platform_error_converts_to_anyhow() {
        let e = PlatformError::Unsupported {
            platform: "Linux".to_string(),
        };
        let _anyhow_err: anyhow::Error = e.into();
    }
}
