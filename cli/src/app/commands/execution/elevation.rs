//! Elevation planning and task-graph pruning.

use std::collections::HashMap;
use std::sync::Arc;

use crate::engine::graph::ResolvedTaskGraph;
use crate::engine::scheduler::ExecutionSummary;
use crate::engine::{Context, Task, TaskAssessment, TaskId, TaskOutcome};
use crate::infra::logging::{ActionCounts, Logger, OutputExt as _, TaskEntry, TaskStatus};

/// Outcome of arranging privilege for the tasks that declared they need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElevationPlan {
    /// Privilege is available to this process; run the tasks normally.
    Ready,
    /// The tasks already ran elsewhere; drop them from this run's graph.
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "only the Windows broker delegates to a child")
    )]
    Delegated,
    /// Privilege could not be arranged; skip the tasks and continue.
    Unavailable { reason: &'static str },
    /// The elevated child ran but failed; fail its tasks and block dependents.
    #[cfg(any(windows, test))]
    Failed { reason: &'static str },
}

/// Application-level broker for platform-specific elevation.
///
/// Low-level sudo and UAC mechanisms remain in [`crate::infra::elevation`].
/// This broker owns task policy: identifying elevating tasks, delegating or
/// priming credentials, recording skipped work, and removing dependents whose
/// prerequisites cannot run.
#[derive(Debug)]
pub(super) struct ElevationBroker<'a> {
    ctx: &'a Context,
    log: &'a Arc<Logger>,
}

impl<'a> ElevationBroker<'a> {
    pub(super) const fn new(ctx: &'a Context, log: &'a Arc<Logger>) -> Self {
        Self { ctx, log }
    }

    /// Prepare elevation and remove tasks already delegated or unable to run.
    pub(super) fn prepare(
        &self,
        tasks: &mut Vec<&dyn Task>,
        assessments: &HashMap<TaskId, TaskAssessment>,
        graph: &ResolvedTaskGraph,
    ) -> ExecutionSummary {
        let mut summary = ExecutionSummary::default();
        let elevating: Vec<&dyn Task> = if self.ctx.execution_policy().elevated_child {
            // The child was spawned precisely to run these tasks; it must not
            // recurse into another elevation request.
            Vec::new()
        } else {
            tasks
                .iter()
                .filter(|task| {
                    assessments
                        .get(&task.task_id())
                        .is_some_and(TaskAssessment::requires_elevation)
                })
                .copied()
                .collect()
        };

        if elevating.is_empty() {
            return summary;
        }

        let names: Vec<&str> = elevating.iter().map(|task| task.name()).collect();
        let selectors: Vec<&str> = elevating.iter().map(|task| task.selector()).collect();
        let plan = prepare_elevation(self.ctx, self.log, &names, &selectors);

        // Delegation is not degradation: the tasks really ran, just in the
        // elevated child, so their dependents must still run here. Only an
        // unavailable plan leaves prerequisites unmet.
        let (reason, cascade, failed) = elevation_plan_disposition(plan);

        let Some(reason) = reason else {
            return summary;
        };
        let roots: HashMap<TaskId, &str> = elevating
            .iter()
            .map(|task| (task.task_id(), task.name()))
            .collect();
        if failed || (cascade && self.ctx.require_complete()) {
            summary.add_failures(roots.len());
        }
        let blocked = if cascade {
            graph.blocked_dependents(&roots)
        } else {
            HashMap::new()
        };

        tasks.retain(|task| {
            let id = task.task_id();
            let message = if roots.contains_key(&id) {
                Some(reason.to_string())
            } else {
                blocked.get(&id).map(|cause| format!("requires {cause}"))
            };
            let Some(message) = message else {
                return true;
            };

            let span = tracing::info_span!("task", name = task.name());
            let _enter = span.enter();
            self.log.debug(message.as_str());
            let task_id = task.log_key();
            let status = if roots.contains_key(&id)
                && (failed || (self.ctx.require_complete() && cascade))
            {
                TaskStatus::Failed
            } else if blocked.contains_key(&id) {
                TaskStatus::Blocked
            } else {
                TaskStatus::Skipped
            };
            self.log.record_task(
                TaskEntry::new(
                    &task_id,
                    task.name(),
                    status,
                    Some(message.as_str()),
                    ActionCounts::default(),
                    task.visibility(),
                )
                .with_selector(task.selector()),
            );
            self.log.mark_task_completed(&task_id);
            self.log.emit_task_result_and_redraw(&task_id);
            summary.record(
                id,
                task.name(),
                if roots.contains_key(&task.task_id()) {
                    if cascade {
                        TaskOutcome::Unmet
                    } else {
                        TaskOutcome::Satisfied
                    }
                } else {
                    TaskOutcome::Blocked
                },
            );
            false
        });
        summary
    }
}

const fn elevation_plan_disposition(plan: ElevationPlan) -> (Option<&'static str>, bool, bool) {
    match plan {
        ElevationPlan::Ready => (None, false, false),
        ElevationPlan::Delegated => (Some("ran in elevated session"), false, false),
        ElevationPlan::Unavailable { reason } => (Some(reason), true, false),
        #[cfg(any(windows, test))]
        ElevationPlan::Failed { reason } => (Some(reason), true, true),
    }
}

/// Arrange privilege for `names`, or report that it is unavailable.
///
/// Prime credentials in the foreground: executor commands run in their own
/// process groups and cannot safely prompt through the controlling terminal.
#[cfg(unix)]
fn prepare_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    _selectors: &[&str],
) -> ElevationPlan {
    prepare_sudo_elevation(
        ctx,
        log,
        names,
        crate::infra::elevation::sudo_available(ctx.executor()),
        crate::infra::elevation::sudo_credentials_cached,
        crate::infra::elevation::prime_sudo_credentials,
    )
}

#[cfg(any(unix, test))]
fn prepare_sudo_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    sudo_available: bool,
    credentials_cached: impl FnOnce() -> bool,
    prime_credentials: impl FnOnce() -> std::io::Result<bool>,
) -> ElevationPlan {
    if !sudo_available {
        log.separate_from_startup();
        log.warn("sudo not found on PATH");
        return ElevationPlan::Unavailable {
            reason: "sudo credentials unavailable",
        };
    }
    log.debug("priming sudo credential cache");

    if credentials_cached() {
        log.debug("sudo credentials already cached");
        return ElevationPlan::Ready;
    }

    if ctx.non_interactive() {
        log.warn(format!(
            "sudo credentials are required for: {}",
            names.join(", ")
        ));
        return ElevationPlan::Unavailable {
            reason: "sudo credentials unavailable in a non-interactive session",
        };
    }

    log.separate_from_startup();
    log.always(format!("sudo is required for: {}", names.join(", ")));
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match prime_credentials() {
        Ok(true) => ElevationPlan::Ready,
        Ok(false) => {
            log.separate_from_startup();
            log.error("sudo credential priming failed");
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable",
            }
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(format!("failed to run sudo: {error:#}"));
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable",
            }
        }
    }
}

/// Delegate the elevating tasks to a single short-lived elevated child run.
///
/// Windows has no per-command `sudo`, so the alternative to one scoped child is
/// elevating the whole run. The child is restricted to `selectors`, so only the
/// tasks that declared `needs_elevation` ever hold an administrator token; this
/// process keeps running unelevated in the user's own terminal.
#[cfg(windows)]
fn prepare_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    selectors: &[&str],
) -> ElevationPlan {
    use crate::infra::elevation::{ElevationOutcome, run_elevated_child};

    if selectors.is_empty() {
        return ElevationPlan::Ready;
    }

    // A UAC consent dialog is drawn on the interactive secure desktop. In CI or
    // any other headless session there is nobody to answer it, so requesting it
    // would at best fail and at worst stall the run until the command timeout.
    // Degrade to the same outcome as a declined prompt instead.
    if !ctx.execution_policy().can_prompt_for_elevation() {
        log.warn(format!(
            "administrator access is required for: {}",
            names.join(", ")
        ));
        return ElevationPlan::Unavailable {
            reason: "elevation unavailable in a non-interactive session",
        };
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let child_args = build_elevated_child_args(&args, selectors);

    log.separate_from_startup();
    log.always(format!(
        "administrator access is required for: {}",
        names.join(", ")
    ));
    log.always("A UAC prompt will open; the rest of this run stays unelevated.");
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match run_elevated_child(ctx.executor(), &**log, &child_args) {
        Ok(ElevationOutcome::Completed) => {
            log.always("Elevated step finished.");
            ElevationPlan::Delegated
        }
        Ok(ElevationOutcome::Declined) => {
            log.separate_from_startup();
            log.warn("elevation declined; continuing without it");
            ElevationPlan::Unavailable {
                reason: "elevation declined",
            }
        }
        Ok(ElevationOutcome::Failed(code)) => {
            log.separate_from_startup();
            log.error(format!("elevated step failed (exit code {code})"));
            ElevationPlan::Failed {
                reason: "elevated step failed",
            }
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(format!("failed to request elevation: {error:#}"));
            ElevationPlan::Unavailable {
                reason: "elevation unavailable",
            }
        }
    }
}

/// Neither `sudo` nor UAC applies; run everything in-process.
#[cfg(not(any(unix, windows)))]
const fn prepare_elevation(
    _ctx: &Context,
    _log: &Arc<Logger>,
    _names: &[&str],
    _selectors: &[&str],
) -> ElevationPlan {
    ElevationPlan::Ready
}

/// Rewrite this run's arguments so the elevated child runs only `selectors`.
///
/// Existing `--only` / `--skip` filters and `--with-deps` are dropped because the
/// parent has already resolved the child's exact scope. `--no-parallel` is
/// forced so output stays readable in the separate console `Start-Process`
/// opens. Every other flag is preserved.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "used by the Windows elevation broker")
)]
pub(super) fn build_elevated_child_args(args: &[String], selectors: &[&str]) -> Vec<String> {
    /// Filters whose values the child must not inherit.
    const DROPPED_WITH_VALUE: [&str; 2] = ["--only", "--skip"];

    let mut out: Vec<String> = Vec::with_capacity(args.len().saturating_add(4));
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if DROPPED_WITH_VALUE.contains(&arg.as_str()) {
            skip_next = true;
            continue;
        }
        if DROPPED_WITH_VALUE
            .iter()
            .any(|flag| arg.starts_with(&format!("{flag}=")))
        {
            continue;
        }
        if matches!(
            arg.as_str(),
            "--no-parallel" | "--elevated-child" | "--with-deps"
        ) {
            continue;
        }
        out.push(arg.clone());
    }

    out.push("--only".to_string());
    out.push(selectors.join(","));
    out.push("--no-parallel".to_string());
    out.push("--elevated-child".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{empty_config, make_static_context};

    #[test]
    fn sudo_primes_foreground_credentials_for_single_and_sequential_tasks() {
        let cases: &[(&str, bool, &[&str])] = &[
            ("single parallel", true, &["System files"]),
            ("single sequential", false, &["System files"]),
            ("multiple sequential", false, &["System files", "Packages"]),
            ("multiple parallel", true, &["System files", "Packages"]),
        ];
        for &(case, parallel, names) in cases {
            let (ctx, log) = make_static_context(empty_config("fixture-root".into()));
            let ctx = ctx.with_parallel(parallel).with_non_interactive(false);
            let primed = std::cell::Cell::new(false);
            let plan = prepare_sudo_elevation(
                &ctx,
                &log,
                names,
                true,
                || false,
                || {
                    primed.set(true);
                    Ok(true)
                },
            );

            assert_eq!(plan, ElevationPlan::Ready, "{case}");
            assert!(
                primed.get(),
                "{case}: uncached credentials must be primed before executor commands run"
            );
        }
    }

    #[test]
    fn sudo_does_not_prompt_when_cached_or_non_interactive() {
        for (cached, non_interactive, expected) in [
            (true, false, ElevationPlan::Ready),
            (true, true, ElevationPlan::Ready),
            (
                false,
                true,
                ElevationPlan::Unavailable {
                    reason: "sudo credentials unavailable in a non-interactive session",
                },
            ),
        ] {
            let (ctx, log) = make_static_context(empty_config("fixture-root".into()));
            let ctx = ctx.with_non_interactive(non_interactive);
            let plan = prepare_sudo_elevation(
                &ctx,
                &log,
                &["System files"],
                true,
                || cached,
                || panic!("cached or non-interactive runs must not prompt"),
            );
            assert_eq!(
                plan, expected,
                "cached={cached}, non_interactive={non_interactive}"
            );
        }
    }

    #[test]
    fn sudo_unavailable_does_not_check_credentials_or_prompt() {
        let (ctx, log) = make_static_context(empty_config("fixture-root".into()));
        let plan = prepare_sudo_elevation(
            &ctx,
            &log,
            &["System files"],
            false,
            || panic!("missing sudo must not be invoked"),
            || panic!("missing sudo must not prompt"),
        );
        assert_eq!(
            plan,
            ElevationPlan::Unavailable {
                reason: "sudo credentials unavailable",
            }
        );
    }

    #[test]
    fn sudo_priming_failure_leaves_elevation_unavailable() {
        for result in [Ok(false), Err(std::io::Error::other("fixture failure"))] {
            let (ctx, log) = make_static_context(empty_config("fixture-root".into()));
            let ctx = ctx.with_parallel(false).with_non_interactive(false);
            let plan =
                prepare_sudo_elevation(&ctx, &log, &["System files"], true, || false, || result);
            assert_eq!(
                plan,
                ElevationPlan::Unavailable {
                    reason: "sudo credentials unavailable",
                }
            );
        }
    }

    #[test]
    fn failed_elevated_run_is_not_treated_as_optional_unavailability() {
        let (reason, cascade, failed) = elevation_plan_disposition(ElevationPlan::Failed {
            reason: "elevated step failed",
        });

        assert_eq!(reason, Some("elevated step failed"));
        assert!(cascade);
        assert!(failed);
    }
}
