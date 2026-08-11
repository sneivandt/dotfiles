use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::infra::env::Env;
use crate::infra::exec::Executor;
use crate::infra::logging::Log;
use crate::infra::platform::Platform;

use super::CancellationToken;
use crate::infra::logging::OutputExt as _;

mod views;

pub(crate) use views::{PathContext, RepoPaths, SystemContext};

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
    /// variable through the injected [`Env`].  Tests can set this explicitly
    /// to pin the value regardless of the environment handle.
    pub is_ci: Option<bool>,
}

/// Shared context for task execution.
///
/// # Choosing an accessor
///
/// Two idioms are supported deliberately, and mixing them arbitrarily is the
/// main source of confusion:
///
/// - **One-off access** — use the flat accessors ([`Context::root`],
///   [`Context::home`], [`Context::executor`], [`Context::platform`]). This is
///   the dominant idiom and the right default.
/// - **Several related reads in one scope** — take a view snapshot first
///   ([`Context::paths`] for filesystem locations, [`Context::system`] for
///   platform and process execution), then read fields off it:
///
///   ```ignore
///   let system = ctx.system();
///   if system.platform().is_windows() && system.which("pwsh") { … }
///   ```
///
/// The views exist to avoid repeated `ctx.` chains and to keep related reads
/// together; they are not a security or encapsulation boundary. Prefer a view
/// once a scope needs two or more values from the same group.
#[derive(Clone)]
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
    /// Whether the process is running inside a CI environment.
    ///
    /// Derived from the `CI` environment variable at construction time (or
    /// supplied directly via [`ContextOpts::is_ci`]) so that tasks can check
    /// this without reading env-globals themselves and tests can inject the
    /// value without mutating process state.
    is_ci: bool,
    /// Read-only access to the process environment.
    ///
    /// Injected rather than read inline so that resources depending on
    /// environment variables (`PATH`, `SHELL`, `DOTFILES_WRAPPER`) can be
    /// tested deterministically without `unsafe` process-global mutation.
    env: Arc<dyn Env>,
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
            .field("is_ci", &self.is_ci)
            .field("env", &"<dyn Env>")
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
    /// `env` supplies the process environment; production callers pass
    /// [`crate::infra::env::system`] while tests pass a
    /// [`MapEnv`](crate::infra::env::MapEnv) handle.
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
        env: Arc<dyn Env>,
        opts: ContextOpts,
    ) -> Result<Self> {
        let home = if platform.is_windows() {
            env.var("USERPROFILE")
                .or_else(|| env.var("HOME"))
                .context("neither USERPROFILE nor HOME environment variable is set")?
        } else {
            env.var("HOME")
                .context("HOME environment variable is not set")?
        };

        let is_ci = opts.is_ci.unwrap_or_else(|| env.var_os("CI").is_some());

        Ok(Self {
            paths: Arc::new(RepoPaths::new(root)),
            overlay,
            platform,
            log,
            dry_run: opts.dry_run,
            home: Arc::new(std::path::PathBuf::from(home)),
            executor,
            parallel: opts.parallel,
            is_ci,
            env,
            cancelled: CancellationToken::new(),
        })
    }

    /// Create a [`Context`] directly from its constituent parts.
    ///
    /// Intended for test helpers and integration-test scaffolding that supply
    /// fully-constructed components rather than deriving them from the
    /// environment.  Prefer [`Context::new`] in production code.
    ///
    /// The environment defaults to the real process environment; call
    /// [`Context::with_env`] to inject a test double.
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
            is_ci: opts.is_ci.unwrap_or(false),
            env: crate::infra::env::system(),
            cancelled: CancellationToken::new(),
        }
    }

    /// Return a copy of this context that reads environment variables from
    /// `env`.
    #[must_use]
    pub fn with_env(&self, env: Arc<dyn Env>) -> Self {
        self.clone_with(|ctx| ctx.env = env)
    }

    /// Read-only access to the process environment.
    ///
    /// Task and resource code must go through this rather than `std::env` so
    /// that behaviour is injectable under test.
    #[must_use]
    pub fn env(&self) -> &Arc<dyn Env> {
        &self.env
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
    pub fn with_dry_run(&self, dry_run: bool) -> Self {
        self.clone_with(|ctx| ctx.dry_run = dry_run)
    }

    /// Create a copy of this context with parallel mode set.
    #[must_use]
    pub fn with_parallel(&self, parallel: bool) -> Self {
        self.clone_with(|ctx| ctx.parallel = parallel)
    }

    /// Create a copy of this context with a different home directory.
    #[must_use]
    pub fn with_home(&self, home: std::path::PathBuf) -> Self {
        self.clone_with(|ctx| ctx.home = Arc::new(home))
    }

    /// Create a copy of this context with the CI flag overridden.
    ///
    /// Used in tests to validate CI-gated task behaviour without mutating
    /// process-global environment variables.
    #[must_use]
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
    /// `tracing::info!(target: "dotfiles::task_result", …)` call in
    /// `flush_and_complete`, causing task statuses to be silently dropped
    /// from the console for any task that called `debug_fmt` during its
    /// `run()`.  The guard has therefore been removed.
    #[inline]
    pub fn debug_fmt(&self, f: impl FnOnce() -> String) {
        if self.log().debug_enabled() {
            self.log().debug(f());
        }
    }

    /// Log a trace message, evaluating the format string lazily.
    ///
    /// Trace messages reach the run log only, never the console, even under
    /// `--verbose`. Use this for internal plumbing detail (batching, parallel
    /// fan-out) that helps when reading a run log but is noise on screen.
    ///
    /// The same `tracing::enabled!` caveat documented on [`Self::debug_fmt`]
    /// applies here: do not add a level guard.
    #[inline]
    pub fn trace_fmt(&self, f: impl FnOnce() -> String) {
        if self.log().debug_enabled() {
            self.log().trace(f());
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
