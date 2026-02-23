use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::config::Config;
use crate::exec::Executor;
use crate::logging::Log;
use crate::operations::{FileSystemOps, SystemFileSystemOps};
use crate::platform::Platform;

/// Shared context for task execution.
pub struct Context {
    /// Configuration loaded from INI files.
    ///
    /// Wrapped in `Arc<RwLock<_>>` so that `ReloadConfig` can atomically
    /// replace the config after a `git pull` while all other tasks see the
    /// updated values.  Use [`Context::config_read`] for read access.
    pub config: Arc<RwLock<Config>>,
    /// Detected platform information.
    pub platform: Arc<Platform>,
    /// Logger for output and task recording.
    pub log: Arc<dyn Log>,
    /// Whether to perform a dry run (preview changes without applying).
    pub dry_run: bool,
    /// User's home directory path.
    pub home: std::path::PathBuf,
    /// Command executor (for testing or real system calls).
    pub executor: Arc<dyn Executor>,
    /// Whether to process resources in parallel using Rayon.
    pub parallel: bool,
    /// Set to `true` by `UpdateRepository` when the repo was actually updated.
    ///
    /// Wrapped in `Arc` so the flag is shared across per-task contexts in
    /// the parallel scheduler.  Checked by `ReloadConfig` to skip
    /// unnecessary reloads.
    pub repo_updated: Arc<AtomicBool>,
    /// Filesystem operation abstraction (injectable for testing).
    pub fs_ops: Arc<dyn FileSystemOps>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("config", &"<Config>")
            .field("platform", &self.platform)
            .field("log", &"<dyn Log>")
            .field("dry_run", &self.dry_run)
            .field("home", &self.home)
            .field("executor", &"<dyn Executor>")
            .field("parallel", &self.parallel)
            .field("repo_updated", &self.repo_updated)
            .field("fs_ops", &"<dyn FileSystemOps>")
            .finish()
    }
}

impl Context {
    /// Creates a new context for task execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the HOME (or USERPROFILE on Windows) environment variable
    /// is not set.
    pub fn new(
        config: Arc<RwLock<Config>>,
        platform: Arc<Platform>,
        log: Arc<dyn Log>,
        dry_run: bool,
        executor: Arc<dyn Executor>,
        parallel: bool,
    ) -> Result<Self> {
        let home = if cfg!(target_os = "windows") {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .map_err(|_| {
                    anyhow::anyhow!("neither USERPROFILE nor HOME environment variable is set")
                })?
        } else {
            std::env::var("HOME")
                .map_err(|_| anyhow::anyhow!("HOME environment variable is not set"))?
        };

        Ok(Self {
            config,
            platform,
            log,
            dry_run,
            home: std::path::PathBuf::from(home),
            executor,
            parallel,
            repo_updated: Arc::new(AtomicBool::new(false)),
            fs_ops: Arc::new(SystemFileSystemOps),
        })
    }

    /// Acquire a shared read lock on the configuration.
    ///
    /// Recovers from a poisoned lock (which can only occur if a previous task
    /// panicked) by consuming the poison and returning the inner value.
    pub fn config_read(&self) -> std::sync::RwLockReadGuard<'_, Config> {
        self.config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Root directory of the dotfiles repository.
    #[must_use]
    pub fn root(&self) -> std::path::PathBuf {
        self.config_read().root.clone()
    }

    /// Symlinks source directory.
    #[must_use]
    pub fn symlinks_dir(&self) -> std::path::PathBuf {
        self.config_read().root.join("symlinks")
    }

    /// Hooks source directory.
    #[must_use]
    pub fn hooks_dir(&self) -> std::path::PathBuf {
        self.config_read().root.join("hooks")
    }

    /// Create a copy of this context with a different logger.
    ///
    /// All other fields are cloned by reference (via `Arc`). This is used by
    /// the parallel scheduler to give each task its own buffered logger while
    /// sharing the rest of the context.
    #[must_use]
    pub fn with_log(&self, log: Arc<dyn Log>) -> Self {
        Self {
            config: Arc::clone(&self.config),
            platform: Arc::clone(&self.platform),
            log,
            dry_run: self.dry_run,
            home: self.home.clone(),
            executor: Arc::clone(&self.executor),
            parallel: self.parallel,
            repo_updated: Arc::clone(&self.repo_updated),
            fs_ops: Arc::clone(&self.fs_ops),
        }
    }

    /// Create a copy of this context with a different [`FileSystemOps`] implementation.
    ///
    /// Used in tests to inject a [`MockFileSystemOps`](crate::operations::MockFileSystemOps)
    /// so that tasks can be exercised without touching the real filesystem.
    #[cfg(test)]
    #[must_use]
    pub fn with_fs_ops(&self, fs_ops: Arc<dyn FileSystemOps>) -> Self {
        Self {
            config: Arc::clone(&self.config),
            platform: Arc::clone(&self.platform),
            log: Arc::clone(&self.log),
            dry_run: self.dry_run,
            home: self.home.clone(),
            executor: Arc::clone(&self.executor),
            parallel: self.parallel,
            repo_updated: Arc::clone(&self.repo_updated),
            fs_ops,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::logging::Logger;
    use crate::operations::MockFileSystemOps;
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[test]
    fn root_returns_config_root() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        assert_eq!(ctx.root(), PathBuf::from("/dotfiles"));
    }

    #[test]
    fn symlinks_dir_returns_root_joined_symlinks() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        assert_eq!(ctx.symlinks_dir(), PathBuf::from("/dotfiles/symlinks"));
    }

    #[test]
    fn hooks_dir_returns_root_joined_hooks() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        assert_eq!(ctx.hooks_dir(), PathBuf::from("/dotfiles/hooks"));
    }

    #[test]
    fn with_log_preserves_other_fields() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let new_log: Arc<dyn Log> = Arc::new(Logger::new(false, "new"));
        let ctx2 = ctx.with_log(new_log);
        assert_eq!(ctx2.root(), ctx.root());
        assert_eq!(ctx2.dry_run, ctx.dry_run);
        assert_eq!(ctx2.home, ctx.home);
        assert_eq!(ctx2.parallel, ctx.parallel);
    }

    #[test]
    fn with_fs_ops_replaces_fs_ops() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let mock = Arc::new(MockFileSystemOps::new());
        let ctx2 = ctx.with_fs_ops(mock);
        assert_eq!(ctx2.root(), ctx.root());
        assert_eq!(ctx2.dry_run, ctx.dry_run);
    }

    #[test]
    fn config_read_returns_config() {
        let config = empty_config(PathBuf::from("/my/root"));
        let ctx = make_linux_context(config);
        let root = ctx.config_read().root.clone();
        assert_eq!(root, PathBuf::from("/my/root"));
    }

    #[test]
    fn debug_format_includes_key_fields() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let debug = format!("{ctx:?}");
        assert!(debug.contains("Context"));
        assert!(debug.contains("dry_run"));
        assert!(debug.contains("home"));
    }
}
