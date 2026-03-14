use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::config::Config;
use crate::exec::Executor;
use crate::logging::Log;
use crate::platform::Platform;

use super::CancellationToken;

// Note: `Platform` is `Copy` (two small fields), so it is stored by value
// rather than behind an `Arc`.  This avoids atomic refcount overhead for a
// type that is cheaper to copy than to reference-count.

/// Boolean flags for context construction.
///
/// Passed to [`Context::new`] to avoid positional `bool` confusion.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContextOpts {
    /// Whether to perform a dry run (preview changes without applying).
    pub dry_run: bool,
    /// Whether to process resources in parallel.
    pub parallel: bool,
    /// Whether the process is running inside a CI environment.
    ///
    /// When `None` (the default), [`Context::new`] reads the `CI` environment
    /// variable.  Tests can set this explicitly to avoid mutating process-global
    /// state.
    pub is_ci: Option<bool>,
}

/// Repository-relative paths derived from a single config snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoPaths {
    /// Root directory of the dotfiles repository.
    pub root: std::path::PathBuf,
    /// Symlinks source directory.
    pub symlinks_dir: std::path::PathBuf,
    /// Hooks source directory.
    pub hooks_dir: std::path::PathBuf,
}

/// Shared context for task execution.
#[derive(Clone)]
pub struct Context {
    /// Configuration loaded from TOML files.
    ///
    /// Private — access via [`Context::config_read`] (read) or
    /// [`Context::config_swap`] (write).  Intentionally not `pub` so that
    /// callers cannot bypass the poisoning-recovery logic or hold the lock
    /// longer than necessary.
    config: Arc<RwLock<Arc<Config>>>,
    /// Detected platform information.
    pub platform: Platform,
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
    /// Whether the process is running inside a CI environment.
    ///
    /// Derived from the `CI` environment variable at construction time (or
    /// supplied directly via [`ContextOpts::is_ci`]) so that tasks can check
    /// this without reading env-globals themselves and tests can inject the
    /// value without mutating process state.
    pub is_ci: bool,
    /// Token for cooperative cancellation (e.g. Ctrl-C).
    ///
    /// Processing loops check this before dispatching each work item so that
    /// in-flight operations finish cleanly and a partial summary is printed.
    pub cancelled: CancellationToken,
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
            .field("is_ci", &self.is_ci)
            .field("cancelled", &self.cancelled)
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
        config: Arc<RwLock<Arc<Config>>>,
        platform: Platform,
        log: Arc<dyn Log>,
        executor: Arc<dyn Executor>,
        opts: ContextOpts,
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

        let is_ci = opts.is_ci.unwrap_or_else(|| std::env::var("CI").is_ok());

        Ok(Self {
            config,
            platform,
            log,
            dry_run: opts.dry_run,
            home: std::path::PathBuf::from(home),
            executor,
            parallel: opts.parallel,
            is_ci,
            cancelled: CancellationToken::new(),
        })
    }

    /// Create a [`Context`] directly from its constituent parts.
    ///
    /// Intended for test helpers and integration-test scaffolding that supply
    /// fully-constructed components rather than deriving them from the
    /// environment.  Prefer [`Context::new`] in production code.
    pub fn from_raw(
        config: Arc<RwLock<Arc<Config>>>,
        platform: Platform,
        log: Arc<dyn Log>,
        executor: Arc<dyn Executor>,
        home: std::path::PathBuf,
        opts: ContextOpts,
    ) -> Self {
        Self {
            config,
            platform,
            log,
            dry_run: opts.dry_run,
            home,
            executor,
            parallel: opts.parallel,
            is_ci: opts.is_ci.unwrap_or(false),
            cancelled: CancellationToken::new(),
        }
    }

    /// Get a snapshot of the current configuration.
    ///
    /// The returned `Arc<Config>` is a cheap clone; the read lock is held
    /// only for the duration of the `Arc::clone`.  Callers can hold the
    /// snapshot as long as needed without blocking `ReloadConfig`.
    #[must_use]
    pub fn config_read(&self) -> Arc<Config> {
        Arc::clone(&*self.config.read().unwrap_or_else(|e| {
            self.log
                .warn(&format!("config read lock was poisoned, recovering: {e}"));
            e.into_inner()
        }))
    }

    /// Repository-relative paths derived from one config snapshot.
    ///
    /// Prefer this over multiple calls to [`Context::root`],
    /// [`Context::symlinks_dir`], and [`Context::hooks_dir`] when the caller
    /// needs more than one path from the same config version.
    #[must_use]
    pub(crate) fn repo_paths(&self) -> RepoPaths {
        let config = self.config_read();
        let root = config.root.clone();
        RepoPaths {
            symlinks_dir: root.join("symlinks"),
            hooks_dir: root.join("hooks"),
            root,
        }
    }

    /// Root directory of the dotfiles repository.
    #[must_use]
    pub fn root(&self) -> std::path::PathBuf {
        self.repo_paths().root
    }

    /// Symlinks source directory.
    #[must_use]
    pub fn symlinks_dir(&self) -> std::path::PathBuf {
        self.repo_paths().symlinks_dir
    }

    /// Hooks source directory.
    #[must_use]
    pub fn hooks_dir(&self) -> std::path::PathBuf {
        self.repo_paths().hooks_dir
    }

    /// Create a copy of this context with a different logger.
    ///
    /// All other fields are cloned by reference (via `Arc`). This is used by
    /// the parallel scheduler to give each task its own buffered logger while
    /// sharing the rest of the context.
    #[must_use]
    pub fn with_log(&self, log: Arc<dyn Log>) -> Self {
        Self {
            config: self.config.clone(),
            platform: self.platform,
            log,
            dry_run: self.dry_run,
            home: self.home.clone(),
            executor: self.executor.clone(),
            parallel: self.parallel,
            is_ci: self.is_ci,
            cancelled: self.cancelled.clone(),
        }
    }

    /// Create a copy of this context with dry-run mode set.
    #[must_use]
    pub fn with_dry_run(&self, dry_run: bool) -> Self {
        Self {
            config: self.config.clone(),
            platform: self.platform,
            log: self.log.clone(),
            dry_run,
            home: self.home.clone(),
            executor: self.executor.clone(),
            parallel: self.parallel,
            is_ci: self.is_ci,
            cancelled: self.cancelled.clone(),
        }
    }

    /// Create a copy of this context with parallel mode set.
    #[must_use]
    pub fn with_parallel(&self, parallel: bool) -> Self {
        Self {
            config: self.config.clone(),
            platform: self.platform,
            log: self.log.clone(),
            dry_run: self.dry_run,
            home: self.home.clone(),
            executor: self.executor.clone(),
            parallel,
            is_ci: self.is_ci,
            cancelled: self.cancelled.clone(),
        }
    }

    /// Create a copy of this context with a different home directory.
    #[must_use]
    pub fn with_home(&self, home: std::path::PathBuf) -> Self {
        Self {
            config: self.config.clone(),
            platform: self.platform,
            log: self.log.clone(),
            dry_run: self.dry_run,
            home,
            executor: self.executor.clone(),
            parallel: self.parallel,
            is_ci: self.is_ci,
            cancelled: self.cancelled.clone(),
        }
    }

    /// Create a copy of this context with the CI flag overridden.
    ///
    /// Used in tests to validate CI-gated task behaviour without mutating
    /// process-global environment variables.
    #[must_use]
    pub fn with_ci(&self, is_ci: bool) -> Self {
        Self {
            config: self.config.clone(),
            platform: self.platform,
            log: self.log.clone(),
            dry_run: self.dry_run,
            home: self.home.clone(),
            executor: self.executor.clone(),
            parallel: self.parallel,
            is_ci,
            cancelled: self.cancelled.clone(),
        }
    }

    /// Create a copy of this context with the given cancellation token.
    ///
    /// Used to wire the signal handler's token into the execution context.
    #[must_use]
    pub fn with_cancellation(&self, cancelled: CancellationToken) -> Self {
        Self {
            config: self.config.clone(),
            platform: self.platform,
            log: self.log.clone(),
            dry_run: self.dry_run,
            home: self.home.clone(),
            executor: self.executor.clone(),
            parallel: self.parallel,
            is_ci: self.is_ci,
            cancelled,
        }
    }

    /// Returns `true` if the process has been asked to shut down.
    ///
    /// Convenience wrapper around `self.cancelled.is_cancelled()`.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.is_cancelled()
    }

    /// Log a debug message, evaluating the format string lazily.
    ///
    /// The closure `f` is only evaluated when debug logging is active for the
    /// current thread, avoiding needless string allocations on true no-op
    /// paths while still keeping hot-path call sites clean.
    ///
    /// # Note on `tracing::enabled!`
    ///
    /// A previous implementation guarded this method with
    /// `tracing::enabled!(Level::DEBUG)` to skip the allocation when the
    /// debug level was disabled.  That check goes through the tracing
    /// per-layer `FilterState` machinery and leaves stale filter-pass bits
    /// on the calling thread.  Those bits interfere with the subsequent
    /// `tracing::info!(target: "dotfiles::stage", …)` call in
    /// `flush_and_complete`, causing stage headers to be silently dropped
    /// from the console for any task that called `debug_fmt` during its
    /// `run()`.  The guard has therefore been removed.
    #[inline]
    pub fn debug_fmt(&self, f: impl FnOnce() -> String) {
        if self.log.debug_enabled() {
            self.log.debug(&f());
        }
    }

    /// Atomically replace the shared configuration.
    ///
    /// Used by [`crate::tasks::reload_config::ReloadConfig`] after a `git pull`
    /// to swap in the freshly-loaded config.
    pub fn config_swap(&self, new_config: Config) {
        let mut guard = self.config.write().unwrap_or_else(|e| {
            self.log
                .warn(&format!("config write lock was poisoned, recovering: {e}"));
            e.into_inner()
        });
        *guard = Arc::new(new_config);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::logging::Logger;
    use crate::logging::{Output, TaskRecorder, TaskStatus};
    use crate::tasks::test_helpers::{empty_config, make_linux_context};
    use std::path::PathBuf;

    #[derive(Debug)]
    struct SilentLog;

    impl Output for SilentLog {
        fn stage(&self, _msg: &str) {}
        fn info(&self, _msg: &str) {}
        fn debug(&self, _msg: &str) {}
        fn warn(&self, _msg: &str) {}
        fn error(&self, _msg: &str) {}
        fn dry_run(&self, _msg: &str) {}
        fn debug_enabled(&self) -> bool {
            false
        }
    }

    impl TaskRecorder for SilentLog {
        fn record_task(&self, _name: &str, _status: TaskStatus, _message: Option<&str>) {}
    }

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
    fn repo_paths_returns_all_derived_paths_from_one_snapshot() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let paths = ctx.repo_paths();
        assert_eq!(paths.root, PathBuf::from("/dotfiles"));
        assert_eq!(paths.symlinks_dir, PathBuf::from("/dotfiles/symlinks"));
        assert_eq!(paths.hooks_dir, PathBuf::from("/dotfiles/hooks"));
    }

    #[test]
    fn with_log_preserves_other_fields() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let new_log: Arc<dyn Log> = Arc::new(Logger::new("new"));
        let ctx2 = ctx.with_log(new_log);
        assert_eq!(ctx2.root(), ctx.root());
        assert_eq!(ctx2.dry_run, ctx.dry_run);
        assert_eq!(ctx2.home, ctx.home);
        assert_eq!(ctx2.parallel, ctx.parallel);
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

    #[test]
    fn clone_shares_arc_fields() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let ctx2 = ctx.clone();
        assert_eq!(ctx2.root(), ctx.root());
        assert_eq!(ctx2.dry_run, ctx.dry_run);
        assert_eq!(ctx2.home, ctx.home);
        assert_eq!(ctx2.parallel, ctx.parallel);
        assert!(Arc::ptr_eq(&ctx.config_read(), &ctx2.config_read()));
        assert_eq!(ctx.platform, ctx2.platform);
    }

    #[test]
    fn debug_fmt_skips_closure_when_debug_logging_is_disabled() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config).with_log(Arc::new(SilentLog));
        let called = std::sync::atomic::AtomicBool::new(false);
        ctx.debug_fmt(|| {
            called.store(true, std::sync::atomic::Ordering::SeqCst);
            "debug message".to_string()
        });
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    }
}
