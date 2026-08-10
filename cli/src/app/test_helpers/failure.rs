//! Failure-injection doubles for exercising error paths.
//!
//! Happy-path doubles are cheap to write, so error paths tend to be
//! under-tested simply because each module has to reinvent a way to make
//! something fail. These helpers make "fail the third `pacman` call" or "fail
//! `apply()` after two successes" a one-liner, and they record what happened so
//! a test can assert that the failure landed where it was expected.
//!
//! Both doubles are `Send + Sync` and use interior mutability, so they can be
//! shared across threads to exercise parallel execution paths.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;

use crate::engine::resource::ResourceError;
use crate::engine::{
    IntrinsicState, RemovableResource, Resource, ResourceChange, ResourceResult, ResourceState,
};
use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor};

/// Which invocation of an injected operation should fail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FailAt {
    /// Never fail; every invocation succeeds.
    #[default]
    Never,
    /// Fail the nth matching invocation (1-based) and succeed otherwise.
    ///
    /// This is the interesting case for partial-failure testing: work before
    /// the nth call has already been performed, so the test observes a
    /// half-applied state rather than a clean no-op.
    Call(usize),
    /// Fail every matching invocation.
    Always,
}

impl FailAt {
    /// Whether the invocation numbered `call` (1-based) should fail.
    #[must_use]
    const fn triggers(self, call: usize) -> bool {
        match self {
            Self::Never => false,
            Self::Call(n) => call == n,
            Self::Always => true,
        }
    }
}

/// Executor double that succeeds until a chosen invocation, then fails.
///
/// Every `run*` method is routed through the same counter so a policy of
/// [`FailAt::Call(2)`](FailAt::Call) fails the second command the code under
/// test issues, regardless of which `run` variant produced it. Restricting the
/// policy to one program with [`FailingExecutor::only_program`] narrows both
/// the counter and the failure to that program.
///
/// # Examples
///
/// ```ignore
/// let exec = Arc::new(FailingExecutor::new(FailAt::Call(2)).only_program("pacman"));
/// // ... run the task under test ...
/// assert_eq!(exec.calls().len(), 3);
/// ```
#[derive(Debug)]
pub struct FailingExecutor {
    fail_at: FailAt,
    only_program: Option<String>,
    stdout: String,
    exit_success: bool,
    which_result: bool,
    calls: Mutex<Vec<String>>,
    matched: AtomicUsize,
}

impl FailingExecutor {
    /// Create an executor that fails according to `fail_at`.
    #[must_use]
    pub fn new(fail_at: FailAt) -> Self {
        Self {
            fail_at,
            only_program: None,
            stdout: String::new(),
            exit_success: true,
            which_result: true,
            calls: Mutex::new(Vec::new()),
            matched: AtomicUsize::new(0),
        }
    }

    /// Only count and fail invocations of `program`.
    #[must_use]
    pub fn only_program(mut self, program: impl Into<String>) -> Self {
        self.only_program = Some(program.into());
        self
    }

    /// Set the stdout returned by successful invocations.
    #[must_use]
    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = stdout.into();
        self
    }

    /// Set the exit status reported by non-injected invocations.
    ///
    /// Use `false` to model a command that ran but reported failure, which is
    /// distinct from an invocation that could not be run at all.
    #[must_use]
    pub const fn with_exit_success(mut self, exit_success: bool) -> Self {
        self.exit_success = exit_success;
        self
    }

    /// Set the value returned by `which()` / `which_path()`.
    #[must_use]
    pub fn with_which(mut self, which_result: bool) -> Self {
        self.which_result = which_result;
        self
    }

    /// Every command line issued so far, in invocation order.
    #[must_use]
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Number of invocations that matched the program filter.
    #[must_use]
    pub fn matched_calls(&self) -> usize {
        self.matched.load(Ordering::SeqCst)
    }

    fn dispatch(&self, spec: &CommandSpec) -> std::result::Result<ExecResult, ExecError> {
        let program = spec.program().to_string_lossy();
        let args = spec
            .arguments()
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>();
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(
                format!("{program} {}", args.join(" "))
                    .trim_end()
                    .to_string(),
            );

        let matches = self
            .only_program
            .as_ref()
            .is_none_or(|filter| filter == program.as_ref());
        if !matches {
            return Ok(self.success());
        }

        let call = self
            .matched
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        if self.fail_at.triggers(call) {
            return Err(ExecError::spawn(
                program.as_ref(),
                std::io::Error::other(format!("injected failure: {program} call {call}")),
            ));
        }
        let result = self.success();
        if spec.is_checked() && !result.success {
            return Err(ExecError::non_zero(program, result));
        }
        Ok(result)
    }

    fn success(&self) -> ExecResult {
        ExecResult {
            stdout: self.stdout.clone(),
            stderr: String::new(),
            success: self.exit_success,
            code: Some(i32::from(!self.exit_success)),
        }
    }
}

impl Executor for FailingExecutor {
    fn execute(&self, spec: CommandSpec) -> std::result::Result<ExecResult, ExecError> {
        self.dispatch(&spec)
    }

    fn which(&self, _program: &str) -> bool {
        self.which_result
    }

    fn which_path(&self, program: &str) -> Result<std::path::PathBuf> {
        if self.which_result {
            Ok(std::path::PathBuf::from(format!("/usr/bin/{program}")))
        } else {
            anyhow::bail!("{program} not found on PATH")
        }
    }
}

/// Resource double that reports a pending change and fails on a chosen apply.
///
/// Unlike an always-failing resource, this lets a test drive a batch where some
/// resources converge and one does not, which is the shape that surfaces
/// partial-application and error-aggregation bugs.
#[derive(Debug)]
pub struct FailingResource {
    description: String,
    state: ResourceState,
    apply_fail_at: FailAt,
    remove_fail_at: FailAt,
    error: ResourceErrorKind,
    applies: AtomicUsize,
    removes: AtomicUsize,
}

/// Which [`ResourceError`] variant an injected resource failure produces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResourceErrorKind {
    /// An untyped `anyhow` error, the most common shape in domain code.
    #[default]
    Other,
    /// A typed command failure.
    CommandFailed,
    /// A typed permission failure.
    PermissionDenied,
}

impl ResourceErrorKind {
    fn build(self, description: &str, call: usize) -> ResourceError {
        match self {
            Self::Other => anyhow::anyhow!("injected failure: {description} call {call}").into(),
            Self::CommandFailed => {
                ResourceError::command_failed("injected", format!("{description} call {call}"))
            }
            Self::PermissionDenied => ResourceError::permission_denied(description),
        }
    }
}

impl FailingResource {
    /// Create a resource that needs changing and fails `apply()` per `fail_at`.
    #[must_use]
    pub fn new(description: impl Into<String>, fail_at: FailAt) -> Self {
        Self {
            description: description.into(),
            state: ResourceState::Missing,
            apply_fail_at: fail_at,
            remove_fail_at: FailAt::Never,
            error: ResourceErrorKind::Other,
            applies: AtomicUsize::new(0),
            removes: AtomicUsize::new(0),
        }
    }

    /// Set the state reported by `current_state()`.
    #[must_use]
    pub fn with_state(mut self, state: ResourceState) -> Self {
        self.state = state;
        self
    }

    /// Set when `remove()` should fail.
    #[must_use]
    pub const fn with_remove_failure(mut self, fail_at: FailAt) -> Self {
        self.remove_fail_at = fail_at;
        self
    }

    /// Set which [`ResourceError`] variant injected failures produce.
    #[must_use]
    pub const fn with_error(mut self, error: ResourceErrorKind) -> Self {
        self.error = error;
        self
    }

    /// Number of times `apply()` has been called.
    #[must_use]
    pub fn apply_calls(&self) -> usize {
        self.applies.load(Ordering::SeqCst)
    }

    /// Number of times `remove()` has been called.
    #[must_use]
    pub fn remove_calls(&self) -> usize {
        self.removes.load(Ordering::SeqCst)
    }
}

impl Resource for FailingResource {
    fn description(&self) -> String {
        self.description.clone()
    }

    fn apply(&self) -> ResourceResult<ResourceChange> {
        let call = self
            .applies
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        if self.apply_fail_at.triggers(call) {
            return Err(self.error.build(&self.description, call));
        }
        Ok(ResourceChange::Applied)
    }
}

impl RemovableResource for FailingResource {
    fn remove(&self) -> ResourceResult<ResourceChange> {
        let call = self
            .removes
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        if self.remove_fail_at.triggers(call) {
            return Err(self.error.build(&self.description, call));
        }
        Ok(ResourceChange::Applied)
    }
}

impl IntrinsicState for FailingResource {
    fn current_state(&self) -> ResourceResult<ResourceState> {
        Ok(self.state.clone())
    }
}
