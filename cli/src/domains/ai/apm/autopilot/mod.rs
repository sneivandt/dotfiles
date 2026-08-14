//! Copilot App workflow autopilot fixup for APM-managed workflows.
//!
//! After `apm install` rewrites workflow rows secure-by-default, this module
//! re-asserts *only the workflows this dotfiles install deployed* to autopilot
//! and enabled, and decides, via a ground-truth pre/post snapshot, whether
//! anything actually changed so that steady-state runs stay quiet.
//!
//! # Scoping to dotfiles-deployed workflows
//!
//! APM's `apm--<owner>--<pkg>--<prompt>` id namespace is shared by *every*
//! apm-deployed workflow on the machine, regardless of which manifest or
//! project deployed it, so a blanket `id GLOB 'apm--*'` update would also flip
//! workflows a user installed through an unrelated `apm install` to autopilot +
//! enabled -- silently arming foreign automations to run on a schedule.  To
//! avoid that, the fixup reads the exact set of workflow ids this install
//! deployed from APM's lockfile (`~/.apm/apm.lock.yaml`), where each deployed
//! workflow is recorded as a `copilot-app-db://workflows/<id>` entry under its
//! dependency's `deployed_files`, and scopes every query to that id set.  When
//! the lockfile lists no workflows (the common case: the deps ship only
//! agents/skills) or is missing, the fixup does nothing.
//!
//! The global lockfile is authoritative here: this task regenerates
//! `~/.apm/apm.yml` from the repo's fragments and runs `apm install -g`
//! immediately before the fixup, so at fixup time the lockfile reflects exactly
//! the dotfiles-managed manifest.  Workflows dropped from the manifest fall out
//! of the lockfile and are intentionally left untouched rather than disabled.

use std::collections::HashSet;

use anyhow::Result;

use crate::engine::Context;
use crate::infra::exec::{CommandSpec, ExecResult};
use crate::infra::logging::OutputExt as _;

mod db;
mod lockfile;
mod outcome;
mod scripts;

use db::{WorkflowDbProbe, probe_workflow_db};
use lockfile::read_deployed_workflow_ids;
use outcome::{FixupExecution, FixupOutcome, interpret_fixup_result, report_fixup_execution};
#[cfg(test)]
pub(super) use scripts::{WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT};
#[cfg(not(test))]
use scripts::{WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT};
use scripts::{build_workflow_script_args, parse_desired_ids};

fn run_workflow_script(
    ctx: &Context,
    python: &str,
    db_str: &str,
    script: &str,
    ids: &[String],
) -> Result<ExecResult> {
    let args = build_workflow_script_args(script, db_str, ids);
    let system = ctx.system();
    Ok(system.executor().execute(
        CommandSpec::new(python)
            .args(&args)
            .current_dir(system.home())
            .unchecked(),
    )?)
}

/// Re-assert that the Copilot App workflows *this dotfiles install deployed*
/// run on autopilot.
///
/// APM installs workflow prompts into the Copilot App's `SQLite` database
/// (`~/.copilot/data.db`) secure-by-default: every row arrives
/// `mode='interactive'` and `enabled=0`, so a freshly installed automation
/// will not fire until a human re-enables it in the App's Workflows tab.  For
/// the dotfiles-managed workflows that is undesirable -- they are meant to be
/// hands-off -- so after a successful `apm install` or `apm update` we
/// flip exactly those rows to `mode='autopilot'` and `enabled=1`.
///
/// The set of dotfiles-managed workflow ids is read fresh from
/// `~/.apm/apm.lock.yaml` (see [`lockfile::read_deployed_workflow_ids`]) -- the
/// lockfile the apm operation we just ran regenerated -- so workflows belonging
/// to other manifests are never touched.  When the lockfile records no
/// workflows (or is missing), there is nothing to do and the fixup returns
/// quietly.
///
/// This is strictly best-effort and never fails the task: APM has already done
/// the real work by the time we get here.  The most common failure is a locked
/// database, which means the Copilot App is currently open and holding the
/// lock; we surface that loudly so the user knows to close the App (or just
/// toggle the workflows by hand).  The update runs through Python's stdlib
/// `sqlite3` module so we do not need a `SQLite` binary on PATH or a Rust
/// `SQLite` dependency.
pub(super) fn apply_workflow_autopilot_fixup(ctx: &Context, pre: &DesiredApmWorkflows) -> bool {
    let Some(ids) = fixup_workflow_ids(ctx) else {
        return false;
    };
    let Some((python, db_str)) = fixup_runtime(ctx) else {
        return false;
    };

    match run_workflow_script(ctx, python, &db_str, WORKFLOW_AUTOPILOT_SCRIPT, &ids) {
        Ok(result) => {
            let execution = interpret_fixup_result(result, pre);
            let changed = matches!(
                &execution,
                FixupExecution::Completed {
                    outcome: FixupOutcome::Set(_),
                    ..
                }
            );
            report_fixup_execution(ctx, execution);
            changed
        }
        Err(e) => {
            ctx.log().warn(format!(
                "autopilot fixup could not run {python} (the apm operation still succeeded): {e:#}"
            ));
            false
        }
    }
}

/// Detect whether any dotfiles-managed Copilot App workflow is not currently
/// configured for autopilot and enabled.
///
/// Returns the pre-fixup workflow snapshot when repair is needed so apply can
/// report the exact number of workflows changed without probing the database a
/// second time. Probe failures remain best-effort and return `None`; a later
/// APM install or update will retry the existing post-deployment fixup.
pub(super) fn detect_workflow_autopilot_drift(ctx: &Context) -> Option<DesiredApmWorkflows> {
    let deployed = read_deployed_workflow_ids(ctx)?;
    if deployed.is_empty() {
        return None;
    }
    let current = snapshot_desired_apm_workflow_ids(ctx);
    match &current {
        DesiredApmWorkflows::Known(desired) if desired.len() == deployed.len() => None,
        DesiredApmWorkflows::Known(_) | DesiredApmWorkflows::FirstInstall => Some(current),
        DesiredApmWorkflows::Unavailable => None,
    }
}

/// Read the exact workflow IDs this dotfiles-managed APM install deployed.
fn fixup_workflow_ids(ctx: &Context) -> Option<Vec<String>> {
    match read_deployed_workflow_ids(ctx) {
        Some(ids) if !ids.is_empty() => Some(ids.into_iter().collect()),
        _ => {
            ctx.debug_fmt(|| {
                "autopilot fixup: ~/.apm/apm.lock.yaml lists no dotfiles-managed workflows; \
                 nothing to enable"
                    .to_string()
            });
            None
        }
    }
}

/// Locate the runtime required to execute the workflow database fixup.
fn fixup_runtime(ctx: &Context) -> Option<(&'static str, String)> {
    match probe_workflow_db(ctx) {
        WorkflowDbProbe::Ready { python, db_str } => Some((python, db_str)),
        WorkflowDbProbe::DbMissing { path } => {
            ctx.debug_fmt(|| format!("skipping autopilot fixup: {path} does not exist"));
            None
        }
        WorkflowDbProbe::DbStatError { path, error } => {
            ctx.debug_fmt(|| format!("skipping autopilot fixup: cannot stat {path}: {error}"));
            None
        }
        WorkflowDbProbe::DbPathNotUtf8 { path } => {
            ctx.log().warn(format!(
                "skipping autopilot fixup: database path {path} is not valid UTF-8"
            ));
            None
        }
        WorkflowDbProbe::PythonMissing => {
            ctx.log().warn(
                "skipping autopilot fixup: neither python3 nor python found in PATH; enable the \
                 apm workflows manually from the Copilot App's Workflows tab",
            );
            None
        }
    }
}

/// Ground-truth snapshot of which dotfiles-managed workflows were already in
/// the desired state (`mode='autopilot'`, `enabled=1`) before `apm install`
/// mutated the Copilot App database.
///
/// Scoped to the workflow ids recorded in the *pre-install* lockfile so the
/// post-install fixup can report a real delta instead of the full set APM
/// resets secure-by-default on every run.  In the steady state the pre- and
/// post-install id sets are identical, so the delta is zero and the run stays
/// quiet.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DesiredApmWorkflows {
    /// The pre-install desired ids were read successfully (possibly empty).
    Known(HashSet<String>),
    /// No `~/.copilot/data.db`, no `workflows` table, or no pre-install
    /// lockfile yet -- a first install, where every workflow the fixup ends up
    /// setting is a genuine change.
    FirstInstall,
    /// The snapshot could not be taken (no Python, locked db, bad UTF-8, ...).
    /// The fixup stays quiet to avoid reporting a change it cannot substantiate.
    Unavailable,
}

/// Read the set of already-desired dotfiles-managed workflow ids before
/// install.
///
/// Scopes to the workflow ids in the pre-install `~/.apm/apm.lock.yaml` so the
/// later delta is computed against the same id space the fixup will manage.
/// Best-effort and read-only: every failure path returns a non-`Known` variant
/// and logs at debug level, never warning, because a missing snapshot must not
/// produce a false "set N workflow(s)" line later.
pub(super) fn snapshot_desired_apm_workflow_ids(ctx: &Context) -> DesiredApmWorkflows {
    let ids: Vec<String> = match read_deployed_workflow_ids(ctx) {
        // No prior lockfile: nothing was managed before, so every workflow the
        // post-install fixup sets is genuinely new.
        None => return DesiredApmWorkflows::FirstInstall,
        // A prior lockfile that deployed no workflows: nothing could have been
        // desired, so an empty known set makes any newly added workflow a real
        // change downstream.
        Some(ids) if ids.is_empty() => return DesiredApmWorkflows::Known(HashSet::new()),
        Some(ids) => ids.into_iter().collect(),
    };

    let (python, db_str) = match probe_workflow_db(ctx) {
        WorkflowDbProbe::Ready { python, db_str } => (python, db_str),
        WorkflowDbProbe::DbMissing { .. } => return DesiredApmWorkflows::FirstInstall,
        WorkflowDbProbe::DbStatError { path, error } => {
            ctx.debug_fmt(|| format!("apm workflow snapshot: cannot stat {path}: {error}"));
            return DesiredApmWorkflows::Unavailable;
        }
        WorkflowDbProbe::DbPathNotUtf8 { path } => {
            ctx.debug_fmt(|| {
                format!("apm workflow snapshot: database path {path} is not valid UTF-8")
            });
            return DesiredApmWorkflows::Unavailable;
        }
        WorkflowDbProbe::PythonMissing => {
            ctx.debug_fmt(|| {
                "apm workflow snapshot: neither python3 nor python found in PATH".to_string()
            });
            return DesiredApmWorkflows::Unavailable;
        }
    };

    match run_workflow_script(ctx, python, &db_str, WORKFLOW_DESIRED_IDS_SCRIPT, &ids) {
        Ok(r) if r.success => DesiredApmWorkflows::Known(parse_desired_ids(&r.stdout)),
        Ok(r) => {
            if r.stderr.contains("no such table") {
                DesiredApmWorkflows::FirstInstall
            } else {
                ctx.debug_fmt(|| {
                    format!(
                        "apm workflow snapshot: query failed (continuing): {}",
                        r.stderr.trim()
                    )
                });
                DesiredApmWorkflows::Unavailable
            }
        }
        Err(e) => {
            ctx.debug_fmt(|| format!("apm workflow snapshot: could not run {python}: {e:#}"));
            DesiredApmWorkflows::Unavailable
        }
    }
}

#[cfg(test)]
mod tests;
