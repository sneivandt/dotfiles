use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::infra::exec::Executor;
use crate::infra::logging::Log;
use crate::infra::platform::Platform;

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
    /// Whether to advance locked dependency versions beyond the declared state.
    ///
    /// Set by the `update` command and left `false` by `install`.  Tasks that
    /// can move past the declared/locked state (currently the APM dependency
    /// refresh) gate that behaviour on this flag so that `install` stays a
    /// pure convergence to the declared state.
    pub advance_versions: bool,
}

/// Repository-relative paths derived from the repository root.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RepoPaths {
    root: std::path::PathBuf,
    symlinks_dir: std::path::PathBuf,
    hooks_dir: std::path::PathBuf,
}

/// Filesystem paths exposed to task code as a focused context view.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PathContext<'a> {
    home: &'a Path,
    repo: &'a RepoPaths,
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
    platform: Platform,
    home: &'a Path,
    executor: &'a Arc<dyn Executor>,
    is_ci: bool,
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

/// Shared context for task execution.
#[derive(Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent execution flags (dry_run, parallel, advance_versions, is_ci) \
              are clearer as separate fields than folded into a state enum"
)]
pub struct Context {
    paths: Arc<RepoPaths>,
    /// Optional path to a private overlay repository.
    ///
    /// Path state fixed at construction time, resolved by the application layer
    /// from CLI arguments, environment, or persisted git config.
    overlay: Option<std::path::PathBuf>,
    platform: Platform,
    log: Arc<dyn Log>,
    dry_run: bool,
    home: Arc<std::path::PathBuf>,
    executor: Arc<dyn Executor>,
    parallel: bool,
    /// Whether to advance locked dependency versions beyond the declared state.
    ///
    /// Set by the `update` command; `false` for `install`.  Gates the APM
    /// dependency refresh (`apm outdated` / `apm update`) so that
    /// `install` converges to the declared state without bumping locked refs.
    advance_versions: bool,
    /// Whether the process is running inside a CI environment.
    ///
    /// Derived from the `CI` environment variable at construction time (or
    /// supplied directly via [`ContextOpts::is_ci`]) so that tasks can check
    /// this without reading env-globals themselves and tests can inject the
    /// value without mutating process state.
    is_ci: bool,
    /// Token for cooperative cancellation (e.g. Ctrl-C).
    ///
    /// Processing loops check this before dispatching each work item so that
    /// in-flight operations finish cleanly and a partial summary is printed.
    cancelled: CancellationToken,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("paths", &self.paths)
            .field("overlay", &self.overlay)
            .field("platform", &self.platform)
            .field("log", &"<dyn Log>")
            .field("dry_run", &self.dry_run)
            .field("home", &self.home)
            .field("executor", &"<dyn Executor>")
            .field("parallel", &self.parallel)
            .field("advance_versions", &self.advance_versions)
            .field("is_ci", &self.is_ci)
            .field("cancelled", &self.cancelled)
            .finish()
    }
}

impl Context {
    fn clone_with(&self, update: impl FnOnce(&mut Self)) -> Self {
        let mut cloned = self.clone();
        update(&mut cloned);
        cloned
    }

    /// Creates a new context for task execution.
    ///
    /// # Errors
    ///
    /// Returns an error if the HOME (or USERPROFILE on Windows) environment variable
    /// is not set.
    pub fn new(
        root: std::path::PathBuf,
        overlay: Option<std::path::PathBuf>,
        platform: Platform,
        log: Arc<dyn Log>,
        executor: Arc<dyn Executor>,
        opts: ContextOpts,
    ) -> Result<Self> {
        let home = if cfg!(target_os = "windows") {
            std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .context("neither USERPROFILE nor HOME environment variable is set")?
        } else {
            std::env::var("HOME").context("HOME environment variable is not set")?
        };

        let is_ci = opts.is_ci.unwrap_or_else(|| std::env::var("CI").is_ok());

        Ok(Self {
            paths: Arc::new(RepoPaths::new(root)),
            overlay,
            platform,
            log,
            dry_run: opts.dry_run,
            home: Arc::new(std::path::PathBuf::from(home)),
            executor,
            parallel: opts.parallel,
            advance_versions: opts.advance_versions,
            is_ci,
            cancelled: CancellationToken::new(),
        })
    }

    /// Create a [`Context`] directly from its constituent parts.
    ///
    /// Intended for test helpers and integration-test scaffolding that supply
    /// fully-constructed components rather than deriving them from the
    /// environment.  Prefer [`Context::new`] in production code.
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn from_raw(
        root: std::path::PathBuf,
        overlay: Option<std::path::PathBuf>,
        platform: Platform,
        log: Arc<dyn Log>,
        executor: Arc<dyn Executor>,
        home: std::path::PathBuf,
        opts: ContextOpts,
    ) -> Self {
        Self {
            paths: Arc::new(RepoPaths::new(root)),
            overlay,
            platform,
            log,
            dry_run: opts.dry_run,
            home: Arc::new(home),
            executor,
            parallel: opts.parallel,
            advance_versions: opts.advance_versions,
            is_ci: opts.is_ci.unwrap_or(false),
            cancelled: CancellationToken::new(),
        }
    }

    /// Repository-relative paths derived from the repository root.
    ///
    /// Prefer this over multiple calls to [`Context::root`],
    /// [`Context::symlinks_dir`], and [`Context::hooks_dir`] when the caller
    /// needs more than one path.
    #[must_use]
    pub(crate) fn repo_paths(&self) -> &RepoPaths {
        self.paths.as_ref()
    }

    /// Path to the optional overlay repository, if one is configured.
    #[must_use]
    pub fn overlay(&self) -> Option<&Path> {
        self.overlay.as_deref()
    }

    /// Return a focused view of filesystem paths used by task code.
    #[must_use]
    pub(crate) fn paths(&self) -> PathContext<'_> {
        PathContext {
            home: self.home.as_path(),
            repo: self.repo_paths(),
        }
    }

    /// Return a focused view of platform and process-execution dependencies.
    #[must_use]
    pub(crate) fn system(&self) -> SystemContext<'_> {
        SystemContext {
            platform: self.platform,
            home: self.home.as_path(),
            executor: &self.executor,
            is_ci: self.is_ci,
        }
    }

    /// Root directory of the dotfiles repository.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.paths.root
    }

    /// Detected platform information.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        self.platform
    }

    /// Logger used for output and task recording.
    #[must_use]
    pub fn log(&self) -> &dyn Log {
        &*self.log
    }

    /// Whether mutations are being previewed rather than applied.
    #[must_use]
    pub const fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// User home directory.
    #[must_use]
    pub fn home(&self) -> &Path {
        self.home.as_path()
    }

    /// Command executor.
    #[must_use]
    pub fn executor(&self) -> &dyn Executor {
        &*self.executor
    }

    /// Clone the shared command executor for resource construction.
    #[must_use]
    pub fn executor_arc(&self) -> Arc<dyn Executor> {
        Arc::clone(&self.executor)
    }

    /// Whether task and resource parallelism is enabled.
    #[must_use]
    pub const fn parallel(&self) -> bool {
        self.parallel
    }

    /// Clone the cooperative cancellation token.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancelled.clone()
    }

    /// Create a copy of this context with a different logger.
    ///
    /// Shared dependencies and immutable paths are cloned by reference (via
    /// `Arc`). This is used by the parallel scheduler to give each task its own
    /// buffered logger while sharing the rest of the context.
    #[must_use]
    pub fn with_log(&self, log: Arc<dyn Log>) -> Self {
        self.clone_with(|ctx| ctx.log = log)
    }

    /// Create a copy of this context with dry-run mode set.
    #[must_use]
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn with_dry_run(&self, dry_run: bool) -> Self {
        self.clone_with(|ctx| ctx.dry_run = dry_run)
    }

    /// Create a copy of this context with parallel mode set.
    #[must_use]
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn with_parallel(&self, parallel: bool) -> Self {
        self.clone_with(|ctx| ctx.parallel = parallel)
    }

    /// Create a copy of this context with version-advancement mode set.
    ///
    /// Used by the `update` command to opt into advancing locked dependency
    /// refs (e.g. `apm update`) that `install` deliberately leaves alone.
    #[must_use]
    pub fn with_advance_versions(&self, advance_versions: bool) -> Self {
        self.clone_with(|ctx| ctx.advance_versions = advance_versions)
    }

    /// Create a copy of this context with a different home directory.
    #[must_use]
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn with_home(&self, home: std::path::PathBuf) -> Self {
        self.clone_with(|ctx| ctx.home = Arc::new(home))
    }

    /// Create a copy of this context with the CI flag overridden.
    ///
    /// Used in tests to validate CI-gated task behaviour without mutating
    /// process-global environment variables.
    #[must_use]
    #[cfg(any(test, feature = "internal-api", doctest))]
    pub fn with_ci(&self, is_ci: bool) -> Self {
        self.clone_with(|ctx| ctx.is_ci = is_ci)
    }

    /// Create a copy of this context with the given cancellation token.
    ///
    /// Used to wire the signal handler's token into the execution context.
    #[must_use]
    pub fn with_cancellation(&self, cancelled: CancellationToken) -> Self {
        self.clone_with(|ctx| ctx.cancelled = cancelled)
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
        if self.log().debug_enabled() {
            self.log().debug(&f());
        }
    }
}

impl RepoPaths {
    fn new(root: std::path::PathBuf) -> Self {
        Self {
            symlinks_dir: root.join("symlinks"),
            hooks_dir: root.join("hooks"),
            root,
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;
    use crate::infra::logging::Logger;
    use crate::infra::logging::{Output, TaskRecorder, TaskStatus};
    use crate::test_helpers::{empty_config, make_linux_context};
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
        fn always(&self, _msg: &str) {}
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
        assert_eq!(ctx.root(), Path::new("/dotfiles"));
    }

    #[test]
    fn path_view_returns_derived_paths() {
        let config = empty_config(PathBuf::from("/dotfiles"));
        let ctx = make_linux_context(config);
        let paths = ctx.paths();
        assert_eq!(paths.root(), Path::new("/dotfiles"));
        assert_eq!(paths.symlinks_dir(), Path::new("/dotfiles/symlinks"));
        assert_eq!(paths.hooks_dir(), Path::new("/dotfiles/hooks"));
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
        assert_eq!(ctx2.dry_run(), ctx.dry_run());
        assert_eq!(ctx2.home(), ctx.home());
        assert_eq!(ctx2.parallel(), ctx.parallel());
    }

    #[test]
    fn root_reflects_construction_value() {
        let config = empty_config(PathBuf::from("/my/root"));
        let ctx = make_linux_context(config);
        assert_eq!(ctx.root(), Path::new("/my/root"));
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
        assert_eq!(ctx2.dry_run(), ctx.dry_run());
        assert_eq!(ctx2.home(), ctx.home());
        assert_eq!(ctx2.parallel(), ctx.parallel());
        assert_eq!(ctx.platform(), ctx2.platform());
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
