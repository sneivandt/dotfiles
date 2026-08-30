//! [`ScriptedExecutor`]: an [`Executor`] test double that replays a fixed,
//! ordered queue of expected `git` calls, panicking clearly on a mismatch, a
//! surprise call, or a leftover step. Prefer
//! [`MockExecutor`](crate::infra::exec::MockExecutor) for open-ended behaviour.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;

use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor};

/// Exact match criteria for one scripted step; `program` is always `"git"`.
#[derive(Debug)]
struct Expect {
    args: Vec<String>,
    cwd: PathBuf,
    checked: bool,
}

impl Expect {
    fn assert_matches(&self, spec: &CommandSpec) {
        let program = spec.program().to_string_lossy().into_owned();
        let args: Vec<String> = spec
            .arguments()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let matches = program == "git"
            && args == self.args
            && spec.working_dir() == Some(self.cwd.as_path())
            && spec.is_checked() == self.checked;
        assert!(
            matches,
            "ScriptedExecutor: expected `git` {:?} in {:?} (checked={}), got `{program}` {args:?} in {:?} (checked={})",
            self.args,
            self.cwd,
            self.checked,
            spec.working_dir(),
            spec.is_checked(),
        );
    }
}

/// One scripted response, consumed in call order.
#[derive(Debug)]
struct Step {
    expect: Option<Expect>,
    result: std::result::Result<ExecResult, ExecError>,
}

/// Executor double consuming an ordered queue of expected commands; see the builders below.
#[derive(Debug, Default)]
pub struct ScriptedExecutor {
    steps: Mutex<VecDeque<Step>>,
}

impl ScriptedExecutor {
    /// Create an empty script.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an unconditional step returning success with `stdout`.
    #[must_use]
    pub fn ok(self, stdout: impl Into<String>) -> Self {
        self.push(None, Ok(ExecResult::success(stdout)))
    }

    /// Queue an unconditional step failing with `error`.
    #[must_use]
    pub fn err(self, error: ExecError) -> Self {
        self.push(None, Err(error))
    }

    /// Queue `count` unconditional steps, each failing with an error from `make_error`.
    #[must_use]
    pub fn err_times(mut self, count: usize, make_error: impl Fn() -> ExecError) -> Self {
        for _ in 0..count {
            self = self.err(make_error());
        }
        self
    }

    /// Queue a step requiring an exact checked `git`/`args`/`cwd` match.
    #[must_use]
    pub fn git(self, cwd: impl Into<PathBuf>, args: &[&str], stdout: impl Into<String>) -> Self {
        self.git_result(cwd, args, Ok(ExecResult::success(stdout)))
    }

    /// Queue an exact checked Git command returning `result` as-is.
    #[must_use]
    pub fn git_result(
        self,
        cwd: impl Into<PathBuf>,
        args: &[&str],
        result: std::result::Result<ExecResult, ExecError>,
    ) -> Self {
        self.exact_with(true, cwd, args, result)
    }

    fn exact_with(
        self,
        checked: bool,
        cwd: impl Into<PathBuf>,
        args: &[&str],
        result: std::result::Result<ExecResult, ExecError>,
    ) -> Self {
        self.push(
            Some(Expect {
                args: args.iter().map(|arg| (*arg).to_string()).collect(),
                cwd: cwd.into(),
                checked,
            }),
            result,
        )
    }

    fn push(
        mut self,
        expect: Option<Expect>,
        result: std::result::Result<ExecResult, ExecError>,
    ) -> Self {
        self.steps
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(Step { expect, result });
        self
    }
}

impl Executor for ScriptedExecutor {
    fn execute(&self, spec: CommandSpec) -> std::result::Result<ExecResult, ExecError> {
        let mut steps = self
            .steps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let step = steps.pop_front().unwrap_or_else(|| {
            panic!(
                "ScriptedExecutor: unexpected call to `{} {:?}` — no more scripted steps",
                spec.program().to_string_lossy(),
                spec.arguments()
            )
        });
        drop(steps);
        if let Some(expect) = &step.expect {
            expect.assert_matches(&spec);
        }
        step.result
    }

    /// Always reports "not found"; not exercised by migrated tests. Use `stub_executor` for `which`-driven tests.
    fn which(&self, _program: &str) -> bool {
        false
    }

    fn which_path(&self, program: &str) -> Result<PathBuf> {
        anyhow::bail!("{program} not found on PATH")
    }
}

impl Drop for ScriptedExecutor {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return; // Do not mask an existing test failure.
        }
        let steps = self
            .steps
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(
            steps.is_empty(),
            "ScriptedExecutor: {} scripted step(s) were never consumed",
            steps.len()
        );
    }
}
