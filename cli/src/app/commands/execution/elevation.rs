//! Elevation planning and task-graph pruning.

use std::collections::HashMap;
use std::sync::Arc;

use crate::engine::graph::ResolvedTaskGraph;
use crate::engine::scheduler::ExecutionSummary;
use crate::engine::{Context, Task, TaskAssessment, TaskId, TaskOutcome};
use crate::infra::logging::{ActionCounts, Logger, OutputExt as _, TaskStatus};

#[cfg(windows)]
use self::prepare_windows_elevation as prepare_elevation;

/// UAC may authenticate a different account; only machine-scoped work is safe.
fn machine_scoped_selector(selector: &str) -> bool {
    selector == "developer-mode"
}

/// Reject manually requested or inherited elevated-child scopes before startup
/// can select a profile, write configuration, or run user-scoped tasks.
pub(crate) fn validate_elevated_child_command(
    command: &crate::app::cli::EngineCommand,
    runtime: &super::super::RuntimePolicy<'_>,
) -> anyhow::Result<()> {
    if !runtime.execution.elevated_child {
        return Ok(());
    }
    let crate::app::cli::EngineCommand::Update { opts, global, .. } = command else {
        anyhow::bail!("elevated children may only update machine-scoped tasks");
    };
    anyhow::ensure!(
        !opts.filters.only.is_empty()
            && opts
                .filters
                .only
                .iter()
                .all(|selector| machine_scoped_selector(selector))
            && opts.filters.skip.is_empty()
            && !opts.with_deps,
        "elevated children require an exact --only developer-mode scope without --skip or --with-deps"
    );
    anyhow::ensure!(
        global
            .root
            .as_deref()
            .is_some_and(std::path::Path::is_absolute)
            && global
                .overlay
                .as_deref()
                .is_none_or(std::path::Path::is_absolute),
        "elevated children require an absolute --root and, when selected, absolute --overlay"
    );
    Ok(())
}

pub(super) fn validate_elevated_tasks(ctx: &Context, tasks: &[&dyn Task]) -> anyhow::Result<()> {
    anyhow::ensure!(
        !ctx.execution_policy().elevated_child
            || tasks
                .iter()
                .all(|task| machine_scoped_selector(task.selector())),
        "elevated children cannot run user-scoped tasks"
    );
    Ok(())
}

/// Outcome of arranging privilege for the tasks that declared they need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ElevationPlan {
    /// Privilege is available to this process; run the tasks normally.
    Ready,
    /// The tasks already ran elsewhere; drop them from this run's graph.
    #[cfg_attr(
        not(any(windows, test)),
        expect(dead_code, reason = "only the Windows broker delegates to a child")
    )]
    Delegated,
    /// Elevation was interrupted by cancellation before completing.
    Interrupted,
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
    profile: Option<&'a str>,
}

impl<'a> ElevationBroker<'a> {
    pub(super) const fn new(
        ctx: &'a Context,
        log: &'a Arc<Logger>,
        profile: Option<&'a str>,
    ) -> Self {
        Self { ctx, log, profile }
    }

    /// Prepare elevation and remove tasks already delegated or unable to run.
    pub(super) fn prepare(
        &self,
        tasks: &mut Vec<&dyn Task>,
        assessments: &HashMap<TaskId, TaskAssessment>,
        graph: &ResolvedTaskGraph,
    ) -> ExecutionSummary {
        self.prepare_with(tasks, assessments, graph, prepare_elevation)
    }

    pub(super) fn prepare_with(
        &self,
        tasks: &mut Vec<&dyn Task>,
        assessments: &HashMap<TaskId, TaskAssessment>,
        graph: &ResolvedTaskGraph,
        arrange: impl FnOnce(&Context, &Arc<Logger>, &[&str], &[&str], Option<&str>) -> ElevationPlan,
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
                    (!self.ctx.platform().is_windows() || machine_scoped_selector(task.selector()))
                        && assessments
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
        let plan = arrange(self.ctx, self.log, &names, &selectors, self.profile);
        let plan = if self.ctx.is_cancelled()
            && matches!(
                plan,
                ElevationPlan::Ready | ElevationPlan::Unavailable { .. }
            ) {
            ElevationPlan::Interrupted
        } else {
            plan
        };
        if plan == ElevationPlan::Interrupted {
            self.ctx.cancellation_token().cancel();
        }

        // Delegation is not degradation: the tasks really ran, just in the
        // elevated child, so their dependents must still run here. Only an
        // unavailable or interrupted plan leaves prerequisites unmet.
        let Some((reason, root_outcome, root_status)) =
            elevation_plan_disposition(plan, self.ctx.require_complete())
        else {
            return summary;
        };
        let roots: HashMap<TaskId, &str> = elevating
            .iter()
            .map(|task| (task.task_id(), task.name()))
            .collect();
        if root_status == TaskStatus::Failed {
            summary.add_failures(roots.len());
        }
        let blocked = if matches!(root_outcome, TaskOutcome::Unmet | TaskOutcome::Cancelled) {
            graph.blocked_dependents(&roots)
        } else {
            HashMap::new()
        };

        tasks.retain(|task| {
            let id = task.task_id();
            let (message, status, outcome) = if roots.contains_key(&id) {
                (reason.to_string(), root_status, root_outcome)
            } else if let Some(cause) = blocked.get(&id) {
                if root_outcome == TaskOutcome::Cancelled {
                    (
                        "cancelled".to_string(),
                        TaskStatus::Interrupted,
                        TaskOutcome::Cancelled,
                    )
                } else {
                    (
                        format!("requires {cause}"),
                        TaskStatus::Blocked,
                        TaskOutcome::Blocked,
                    )
                }
            } else {
                return true;
            };

            let span = tracing::info_span!("task", name = task.name());
            let _enter = span.enter();
            self.log.debug(message.as_str());
            let task_id = task.log_key();
            self.log.record_task(crate::engine::task::task_entry(
                *task,
                &task_id,
                status,
                Some(message.as_str()),
                ActionCounts::default(),
            ));
            self.log.mark_task_completed(&task_id);
            self.log.emit_task_result_and_redraw(&task_id);
            summary.record(id, task.name(), outcome);
            false
        });
        summary
    }
}

const fn elevation_plan_disposition(
    plan: ElevationPlan,
    require_complete: bool,
) -> Option<(&'static str, TaskOutcome, TaskStatus)> {
    match plan {
        ElevationPlan::Ready => None,
        ElevationPlan::Delegated => Some((
            "ran in elevated session",
            TaskOutcome::Satisfied,
            TaskStatus::Skipped,
        )),
        ElevationPlan::Interrupted => {
            Some(("cancelled", TaskOutcome::Cancelled, TaskStatus::Interrupted))
        }
        ElevationPlan::Unavailable { reason } => Some((
            reason,
            TaskOutcome::Unmet,
            if require_complete {
                TaskStatus::Failed
            } else {
                TaskStatus::Skipped
            },
        )),
        #[cfg(any(windows, test))]
        ElevationPlan::Failed { reason } => Some((reason, TaskOutcome::Unmet, TaskStatus::Failed)),
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
    _profile: Option<&str>,
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
    log.separate_console_block();
    log.sudo_required(format!("sudo required · {}", names.join(", ")));
    drop(std::io::Write::flush(&mut std::io::stdout()));

    let result = prime_credentials();
    log.separate_console_block();

    if ctx.is_cancelled() {
        return ElevationPlan::Interrupted;
    }

    match result {
        Ok(true) => ElevationPlan::Ready,
        Ok(false) => {
            log.separate_from_startup();
            log.error("could not authenticate with sudo");
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
/// machine-scoped tasks that declared `needs_elevation` hold an administrator token; this
/// process keeps running unelevated in the user's own terminal.
#[cfg(windows)]
fn prepare_windows_elevation(
    ctx: &Context,
    log: &Arc<Logger>,
    names: &[&str],
    selectors: &[&str],
    profile: Option<&str>,
) -> ElevationPlan {
    use crate::infra::elevation::run_elevated_child;

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

    let args = match collect_utf8_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(invalid) => {
            log.separate_from_startup();
            log.error(format!(
                "cannot pass non-UTF-8 argument to elevated child: {}",
                invalid.to_string_lossy()
            ));
            return ElevationPlan::Failed {
                reason: "elevated child arguments contain non-UTF-8 text",
            };
        }
    };
    let resolved_paths = std::path::absolute(ctx.root()).and_then(|root| {
        ctx.overlay()
            .map(std::path::absolute)
            .transpose()
            .map(|overlay| (root, overlay))
    });
    let (root, overlay) = match resolved_paths {
        Ok(paths) => paths,
        Err(error) => {
            log.error(format!("failed to resolve elevated child paths: {error}"));
            return ElevationPlan::Failed {
                reason: "elevated child paths unavailable",
            };
        }
    };
    let child_args =
        build_elevated_child_args(&args, selectors, &root, overlay.as_deref(), profile);

    log.separate_from_startup();
    log.always(format!(
        "administrator access is required for: {}",
        names.join(", ")
    ));
    log.always("A UAC prompt will open; the rest of this run stays unelevated.");
    drop(std::io::Write::flush(&mut std::io::stdout()));

    windows_elevation_result(log, run_elevated_child(ctx.executor(), &**log, &child_args))
}

#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "used by the Windows elevation broker")
)]
fn collect_utf8_args(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Vec<String>, std::ffi::OsString> {
    args.into_iter()
        .map(std::ffi::OsString::into_string)
        .collect()
}

#[cfg(any(windows, test))]
pub(super) fn windows_elevation_result(
    log: &Logger,
    result: anyhow::Result<crate::infra::elevation::ElevationOutcome>,
) -> ElevationPlan {
    use crate::infra::elevation::ElevationOutcome;

    match result {
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
        Ok(ElevationOutcome::Failed(130)) => {
            log.separate_from_startup();
            log.warn("elevated step interrupted");
            ElevationPlan::Interrupted
        }
        Ok(ElevationOutcome::Failed(code)) => {
            log.separate_from_startup();
            log.error(format!("elevated step failed (exit code {code})"));
            ElevationPlan::Failed {
                reason: "elevated step failed",
            }
        }
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<crate::infra::exec::ExecError>()
                    .is_some_and(crate::infra::exec::ExecError::is_cancelled)
            }) =>
        {
            log.separate_from_startup();
            log.warn("elevated step interrupted");
            ElevationPlan::Interrupted
        }
        Err(error) => {
            log.separate_from_startup();
            log.error(format!("failed to request elevation: {error:#}"));
            ElevationPlan::Failed {
                reason: "failed to request elevation",
            }
        }
    }
}

/// Neither `sudo` nor UAC applies; run everything in-process.
#[cfg(not(any(unix, windows)))]
const fn prepare_elevation_fallback(
    _ctx: &Context,
    _log: &Arc<Logger>,
    _names: &[&str],
    _selectors: &[&str],
    _profile: Option<&str>,
) -> ElevationPlan {
    ElevationPlan::Ready
}

#[cfg(not(any(unix, windows)))]
use prepare_elevation_fallback as prepare_elevation;

/// Rewrite this run's arguments so the elevated child runs only `selectors`.
///
/// Existing `--only` / `--skip` filters and `--with-deps` are dropped because the
/// parent has already resolved the child's exact scope. `--no-parallel` is
/// forced so output stays readable in the separate console `Start-Process`
/// opens. Repository paths and, when resolved, `--profile` are replaced with
/// the parent's resolved values.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "used by the Windows elevation broker")
)]
pub(super) fn build_elevated_child_args(
    args: &[String],
    selectors: &[&str],
    root: &std::path::Path,
    overlay: Option<&std::path::Path>,
    profile: Option<&str>,
) -> Vec<String> {
    /// Filters whose values the child must not inherit.
    const DROPPED_WITH_VALUE: [&str; 4] = ["--only", "--skip", "--root", "--overlay"];

    let mut out: Vec<String> = Vec::with_capacity(args.len().saturating_add(6));
    let mut trailing: Vec<String> = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            trailing.push(arg.clone());
            trailing.extend(args.cloned());
            break;
        }
        if DROPPED_WITH_VALUE.contains(&arg.as_str()) {
            let _inherited_value = args.next();
            continue;
        }
        if arg
            .split_once('=')
            .is_some_and(|(flag, _)| DROPPED_WITH_VALUE.contains(&flag))
        {
            continue;
        }
        if profile.is_some() && strip_profile_arg(arg, &mut args, &mut out) {
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

    if let Some(profile) = profile {
        out.push("--profile".to_string());
        out.push(profile.to_string());
    }
    out.push("--only".to_string());
    out.push(selectors.join(","));
    out.push("--no-parallel".to_string());
    out.push("--elevated-child".to_string());
    out.push("--root".to_string());
    out.push(root.display().to_string());
    if let Some(overlay) = overlay {
        out.push("--overlay".to_string());
        out.push(overlay.display().to_string());
    }
    out.extend(trailing);
    out
}

#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "used by the Windows elevation broker")
)]
fn strip_profile_arg(
    arg: &str,
    rest: &mut std::slice::Iter<'_, String>,
    out: &mut Vec<String>,
) -> bool {
    if arg == "--profile" || arg == "-p" {
        let _inherited_value = rest.next();
        return true;
    }
    if arg.starts_with("--profile=") || arg.starts_with("-p=") {
        return true;
    }
    if let Some(cluster) = arg.strip_prefix('-')
        && !cluster.starts_with('-')
        && let Some((prefix, rest_after_p)) = cluster.split_once('p')
    {
        if !prefix.is_empty() {
            out.push(format!("-{prefix}"));
        }
        if rest_after_p.is_empty() {
            let _inherited_value = rest.next();
        }
        return true;
    }
    false
}

#[cfg(test)]
#[path = "tests/elevation_unit.rs"]
mod tests;
