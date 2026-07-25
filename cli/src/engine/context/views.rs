//! Focused read-only views over [`Context`](super::Context).
//!
//! Grouping repository paths and platform/execution access into small snapshot
//! types keeps [`Context`](super::Context) itself limited to ownership and
//! lifecycle concerns, and lets a caller take several related reads in one go.

use std::path::Path;
use std::sync::Arc;

use crate::infra::exec::Executor;
use crate::infra::platform::Platform;

/// Repository-relative paths derived from the repository root.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RepoPaths {
    pub(super) root: std::path::PathBuf,
    pub(super) symlinks_dir: std::path::PathBuf,
    pub(super) hooks_dir: std::path::PathBuf,
}

/// Filesystem paths exposed to task code as a focused context view.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PathContext<'a> {
    pub(super) home: &'a Path,
    pub(super) repo: &'a RepoPaths,
}

impl PathContext<'_> {
    /// User home directory.
    #[must_use]
    pub(crate) const fn home(&self) -> &Path {
        self.home
    }

    /// Dotfiles repository root.
    #[must_use]
    pub(crate) fn root(&self) -> &Path {
        &self.repo.root
    }

    /// Symlink source directory inside the repository.
    #[must_use]
    pub(crate) fn symlinks_dir(&self) -> &Path {
        &self.repo.symlinks_dir
    }

    /// Git hook source directory inside the repository.
    #[must_use]
    pub(crate) fn hooks_dir(&self) -> &Path {
        &self.repo.hooks_dir
    }
}

/// Platform and process-execution access exposed as a focused context view.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SystemContext<'a> {
    pub(super) platform: Platform,
    pub(super) home: &'a Path,
    pub(super) executor: &'a Arc<dyn Executor>,
    pub(super) is_ci: bool,
}

impl SystemContext<'_> {
    /// Detected platform.
    #[must_use]
    pub(crate) const fn platform(&self) -> Platform {
        self.platform
    }

    /// User home directory.
    #[must_use]
    pub(crate) const fn home(&self) -> &Path {
        self.home
    }

    /// Shared command executor.
    #[must_use]
    pub(crate) fn executor(&self) -> &dyn Executor {
        self.executor.as_ref()
    }

    /// Clone the shared command executor for resource construction.
    #[must_use]
    pub(crate) fn executor_arc(&self) -> Arc<dyn Executor> {
        Arc::clone(self.executor)
    }

    /// Return whether the process is running in CI.
    #[must_use]
    pub(crate) const fn is_ci(&self) -> bool {
        self.is_ci
    }

    /// Return whether `program` is available on PATH.
    #[must_use]
    pub(crate) fn which(&self, program: &str) -> bool {
        self.executor.which(program)
    }
}

impl RepoPaths {
    pub(super) fn new(root: std::path::PathBuf) -> Self {
        Self {
            symlinks_dir: root.join("symlinks"),
            hooks_dir: root.join("hooks"),
            root,
        }
    }
}
