//! Elevation planning and task-graph pruning.

use std::collections::HashMap;
use std::sync::Arc;

use crate::engine::{Context, Task, TaskId};
use crate::infra::logging::{ActionCounts, Logger, OutputExt as _, TaskStatus};

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
    pub(super) fn prepare(&self, tasks: &mut Vec<&dyn Task>) {
        let elevating: Vec<&dyn Task> = if crate::infra::elevation::is_elevated_child() {
            // The child was spawned precisely to run these tasks; it must not
            // recurse into another elevation request.
            Vec::new()
        } else {
            tasks
                .iter()
                .filter(|task| crate::engine::requires_elevation(**task, self.ctx))
                .copied()
                .collect()
        };

        if elevating.is_empty() {
            return;
        }

        let names: Vec<&str> = elevating.iter().map(|task| task.name()).collect();
        let selectors: Vec<&str> = elevating.iter().map(|task| task.selector()).collect();
        let plan = prepare_elevation(self.ctx, self.log, &names, &selectors, tasks.len());

        // Delegation is not degradation: the tasks really ran, just in the
        // elevated child, so their dependents must still run here. Only an
        // unavailable plan leaves prerequisites unmet.
        let (reason, cascade) = match plan {
            ElevationPlan::Ready => (None, false),
            ElevationPlan::Delegated => (Some("ran in elevated session"), false),
            ElevationPlan::Unavailable { reason } => (Some(reason), true),
        };

        let Some(reason) = reason else {
            return;
        };

        let roots: HashMap<TaskId, &str> = elevating
            .iter()
            .map(|task| (task.task_id(), task.name()))
            .collect();
        let blocked = if cascade {
            blocked_dependents(tasks, &roots)
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
            self.log.record_task_with_metadata(
                task.name(),
                TaskStatus::Skipped,
                Some(message.as_str()),
                ActionCounts::default(),
                task.visibility(),
            );
            self.log.mark_task_completed(task.name());
            self.log.emit_task_result_and_redraw(task.name());
            false
        });
    }
}

/// Arrange privilege for `names`, or report that it is unavailable.
///
/// Unix keeps the existing behaviour: prime the `sudo` credential cache once so
/// parallel tasks do not interleave password prompts, and let sequential runs
/// prompt inline as they always have.
#[cfg(unix)]
fn prepare_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    _selectors: &[&str],
    task_count: usize,
) -> ElevationPlan {
    // A single task, or a sequential run, can prompt inline without garbling
    // output, so there is nothing to arrange up front.
    if !ctx.parallel() || task_count <= 1 {
        return ElevationPlan::Ready;
    }

    if !crate::infra::elevation::sudo_available(ctx.executor()) {
        log.separate_from_startup();
        log.warn("sudo not found on PATH");
        return ElevationPlan::Unavailable {
            reason: "sudo credentials unavailable",
        };
    }
    log.debug("priming sudo credential cache");

    if crate::infra::elevation::sudo_credentials_cached() {
        log.debug("sudo credentials already cached");
        return ElevationPlan::Ready;
    }

    log.separate_from_startup();
    log.always(format!("sudo is required for: {}", names.join(", ")));
    drop(std::io::Write::flush(&mut std::io::stdout()));

    match crate::infra::elevation::prime_sudo_credentials() {
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
    _task_count: usize,
) -> ElevationPlan {
    use crate::infra::elevation::{ElevationOutcome, run_elevated_child};

    if selectors.is_empty() {
        return ElevationPlan::Ready;
    }

    // A UAC consent dialog is drawn on the interactive secure desktop. In CI or
    // any other headless session there is nobody to answer it, so requesting it
    // would at best fail and at worst stall the run until the command timeout.
    // Degrade to the same outcome as a declined prompt instead.
    if ctx.system().is_ci() || !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
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
            ElevationPlan::Unavailable {
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
    _task_count: usize,
) -> ElevationPlan {
    ElevationPlan::Ready
}

/// Rewrite this run's arguments so the elevated child runs only `selectors`.
///
/// Existing `--only` / `--skip` filters are dropped because the child's scope is
/// decided here, and `--no-parallel` is forced so its output stays readable in
/// the separate console `Start-Process` opens. Every other flag is preserved.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "used by the Windows elevation broker")
)]
pub(super) fn build_elevated_child_args(args: &[String], selectors: &[&str]) -> Vec<String> {
    /// Filters whose values the child must not inherit.
    const DROPPED: [&str; 2] = ["--only", "--skip"];

    let mut out: Vec<String> = Vec::with_capacity(args.len().saturating_add(4));
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if DROPPED.contains(&arg.as_str()) {
            skip_next = true;
            continue;
        }
        if DROPPED
            .iter()
            .any(|flag| arg.starts_with(&format!("{flag}=")))
        {
            continue;
        }
        if arg == "--no-parallel" || arg == "--elevated-child" {
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

/// Tasks that cannot run because a prerequisite in `roots` will not run.
///
/// Returns each blocked task mapped to the name of the root that blocks it.
pub(super) fn blocked_dependents<'task>(
    tasks: &[&'task dyn Task],
    roots: &HashMap<TaskId, &'task str>,
) -> HashMap<TaskId, &'task str> {
    let mut blocked: HashMap<TaskId, &str> = HashMap::new();
    loop {
        let mut discovered = false;
        for task in tasks {
            let id = task.task_id();
            if roots.contains_key(&id) || blocked.contains_key(&id) {
                continue;
            }
            let cause = task
                .dependencies()
                .iter()
                .find_map(|dep| roots.get(dep).or_else(|| blocked.get(dep)).copied());
            if let Some(cause) = cause {
                blocked.insert(id, cause);
                discovered = true;
            }
        }
        if !discovered {
            return blocked;
        }
    }
}
