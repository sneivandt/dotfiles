//! Install missing extensions and reconcile the requests against one final inventory.
use std::collections::{BTreeMap, HashSet};

use anyhow::{Context as _, Result};

use super::launcher::{get_installed_extensions, request_install};
use crate::engine::{
    BatchCompletion, BatchReport, Context, Operation, OperationState, TaskResult, TaskStats,
};
use crate::infra::exec::ExecError;

pub(super) struct ExtensionInstall<'a> {
    pub(super) cmd: &'a str,
    pub(super) extensions: &'a [String],
}

pub(super) struct InstallPlan<'a> {
    current: Vec<&'a str>,
    missing: Vec<&'a str>,
}

type Failures<'a> = BTreeMap<&'a str, String>;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn cancelled(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ExecError>()
        .is_some_and(ExecError::is_cancelled)
}

fn install_action(ctx: &Context, id: &str, planned: bool) {
    ctx.log()
        .action("install", id, planned, &format!("install {id}"));
}

impl InstallPlan<'_> {
    fn stats(&self, changed: usize) -> TaskStats {
        TaskStats::from_counts(count(changed), count(self.current.len()), 0, 0)
    }

    fn verified(
        &self,
        ctx: &Context,
        observed: &HashSet<String>,
        mut failures: Failures<'_>,
    ) -> TaskResult {
        let present = |id: &str| observed.contains(&id.to_lowercase());
        let mut absent: Vec<_> = self
            .current
            .iter()
            .copied()
            .filter(|id| !present(id))
            .collect();
        let already_ok = self.current.len() - absent.len();
        let mut applied = 0;
        for &id in &self.missing {
            if !present(id) {
                absent.push(id);
            } else if let Some(reason) = failures.get_mut(id) {
                reason.push_str(" (listed as installed despite the command failure)");
            } else {
                applied += 1;
                install_action(ctx, id, false);
            }
        }
        let failed = self.current.len() + self.missing.len() - already_ok - applied;
        let stats = TaskStats::from_counts(count(applied), count(already_ok), 0, count(failed));
        let mut reasons: Vec<_> = failures.into_values().collect();
        if !absent.is_empty() {
            reasons.push(format!(
                "VS Code did not report these extensions after installation: {}",
                absent.join(", ")
            ));
        }
        if reasons.is_empty() {
            stats.finish()
        } else {
            stats.with_message(reasons.join("; ")).finish()
        }
    }

    // Requests that exited zero are still awaiting verification. On cancellation
    // they are interrupted work; on inspection failure their verification failed.
    fn stopped(
        &self,
        mut error: anyhow::Error,
        failures: Failures<'_>,
        interrupted: bool,
        not_attempted: usize,
    ) -> anyhow::Error {
        let completion = if interrupted && failures.is_empty() {
            BatchCompletion::Interrupted
        } else {
            BatchCompletion::Failed
        };
        let (failed, unfinished) = if interrupted {
            (
                failures.len(),
                self.missing.len() - not_attempted - failures.len(),
            )
        } else {
            (self.missing.len() - not_attempted, 0)
        };
        let stats = TaskStats::from_counts(0, count(self.current.len()), 0, count(failed));
        for reason in failures.into_values() {
            error = error.context(reason);
        }
        error.context(BatchReport::new(
            stats,
            completion,
            count(unfinished),
            count(not_attempted),
        ))
    }
}

impl<'a> Operation for ExtensionInstall<'a> {
    type Plan = InstallPlan<'a>;

    fn current_state(&self, ctx: &Context) -> Result<OperationState<Self::Plan>> {
        let installed = get_installed_extensions(self.cmd, ctx.executor())
            .context("could not read the VS Code extension inventory")?;
        let mut seen = HashSet::new();
        let (current, missing) = self
            .extensions
            .iter()
            .map(String::as_str)
            .filter(|id| seen.insert(id.to_lowercase()))
            .partition::<Vec<_>, _>(|id| installed.contains(&id.to_lowercase()));
        for id in &current {
            ctx.debug_fmt(|| format!("ok {id}"));
        }
        Ok(OperationState::needs_run(InstallPlan { current, missing }))
    }

    fn preview(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        for id in &plan.missing {
            install_action(ctx, id, true);
        }
        Ok(plan.stats(plan.missing.len()).finish())
    }

    fn apply(&self, ctx: &Context, plan: &Self::Plan) -> Result<TaskResult> {
        if plan.missing.is_empty() {
            return Ok(plan.stats(0).finish());
        }
        let mut failures = Failures::new();
        for (index, &id) in plan.missing.iter().enumerate() {
            let remaining = plan.missing.len() - index;
            if ctx.is_cancelled() {
                return Err(plan.stopped(
                    anyhow::anyhow!("extension installation cancelled"),
                    failures,
                    true,
                    remaining,
                ));
            }
            if let Err(error) = request_install(self.cmd, id, ctx.executor()) {
                if cancelled(&error) {
                    return Err(plan.stopped(error, failures, true, remaining - 1));
                }
                failures.insert(id, format!("{id}: {error:#}"));
            }
        }
        if ctx.is_cancelled() {
            return Err(plan.stopped(
                anyhow::anyhow!("extension verification cancelled"),
                failures,
                true,
                0,
            ));
        }
        match get_installed_extensions(self.cmd, ctx.executor())
            .context("could not verify which VS Code extensions were installed")
        {
            Ok(observed) => Ok(plan.verified(ctx, &observed, failures)),
            Err(error) => {
                let interrupted = cancelled(&error);
                Err(plan.stopped(error, failures, interrupted, 0))
            }
        }
    }
}
