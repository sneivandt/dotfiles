//! [`ScriptedExecutor`]: an [`Executor`] test double that replays a fixed,
//! ordered queue of expected commands, panicking clearly on a mismatch, a
//! surprise call, or a leftover step. Prefer
//! [`MockExecutor`](crate::infra::exec::MockExecutor) for open-ended behaviour.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;

use crate::infra::exec::{CommandSpec, ExecError, ExecResult, Executor};

/// One scripted response, consumed in call order.
#[derive(Debug)]
struct Step {
    expect: Option<CommandSpec>,
    result: std::result::Result<ExecResult, ExecError>,
}

/// Executor double consuming an ordered queue of expected commands; see the builders below.
#[derive(Debug, Default)]
pub struct ScriptedExecutor {
    steps: Mutex<VecDeque<Step>>,
    programs: Vec<String>,
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

    /// Queue a step requiring an exact checked `git`/`args`/`cwd` match.
    #[must_use]
    pub fn git(self, cwd: impl Into<PathBuf>, args: &[&str], stdout: impl Into<String>) -> Self {
        self.command(
            CommandSpec::new("git").args(args).current_dir(cwd),
            Ok(ExecResult::success(stdout)),
        )
    }

    /// Queue a command, checking its program, arguments, cwd and checked mode.
    #[must_use]
    pub fn command(
        self,
        spec: CommandSpec,
        result: std::result::Result<ExecResult, ExecError>,
    ) -> Self {
        self.push(Some(spec), result)
    }

    /// Report only these programs as available to `which`.
    #[must_use]
    pub fn with_programs(mut self, programs: &[&str]) -> Self {
        self.programs = programs
            .iter()
            .map(|program| (*program).to_string())
            .collect();
        self
    }

    fn push(
        mut self,
        expect: Option<CommandSpec>,
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
            assert_eq!(spec.program(), expect.program(), "{spec:?}");
            assert_eq!(spec.arguments(), expect.arguments(), "{spec:?}");
            assert_eq!(spec.working_dir(), expect.working_dir(), "{spec:?}");
            assert_eq!(spec.is_checked(), expect.is_checked(), "{spec:?}");
        }
        step.result
    }

    fn which(&self, program: &str) -> bool {
        self.programs.iter().any(|available| available == program)
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
