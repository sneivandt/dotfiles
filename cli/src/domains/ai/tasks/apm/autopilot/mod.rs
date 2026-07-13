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

use crate::engine::Context;

mod db;
mod lockfile;
mod outcome;
mod scripts;

use db::{WorkflowDbProbe, probe_workflow_db};
use lockfile::read_deployed_workflow_ids;
use outcome::{decide_fixup_outcome, report_fixup_outcome};
#[cfg(test)]
pub(super) use scripts::{WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT};
#[cfg(not(test))]
use scripts::{WORKFLOW_AUTOPILOT_SCRIPT, WORKFLOW_DESIRED_IDS_SCRIPT};
use scripts::{build_workflow_script_args, parse_desired_ids};

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
pub(super) fn apply_workflow_autopilot_fixup(ctx: &Context, pre: &DesiredApmWorkflows) {
    let ids: Vec<String> = match read_deployed_workflow_ids(ctx) {
        Some(ids) if !ids.is_empty() => ids.into_iter().collect(),
        _ => {
            ctx.debug_fmt(|| {
                "autopilot fixup: ~/.apm/apm.lock.yaml lists no dotfiles-managed workflows; \
                 nothing to enable"
                    .to_string()
            });
            return;
        }
    };

    let (python, db_str) = match probe_workflow_db(ctx) {
        WorkflowDbProbe::Ready { python, db_str } => (python, db_str),
        WorkflowDbProbe::DbMissing { path } => {
            ctx.debug_fmt(|| format!("skipping autopilot fixup: {path} does not exist"));
            return;
        }
        WorkflowDbProbe::DbStatError { path, error } => {
            ctx.debug_fmt(|| format!("skipping autopilot fixup: cannot stat {path}: {error}"));
            return;
        }
        WorkflowDbProbe::DbPathNotUtf8 { path } => {
            ctx.log.warn(&format!(
                "skipping autopilot fixup: database path {path} is not valid UTF-8"
            ));
            return;
        }
        WorkflowDbProbe::PythonMissing => {
            ctx.log.warn(
                "skipping autopilot fixup: neither python3 nor python found in PATH; enable the \
                 apm workflows manually from the Copilot App's Workflows tab",
            );
            return;
        }
    };

    let args = build_workflow_script_args(WORKFLOW_AUTOPILOT_SCRIPT, &db_str, &ids);
    match ctx.executor.run_unchecked_in(&ctx.home, python, &args) {
        Ok(r) if r.success => {
            report_fixup_outcome(ctx, decide_fixup_outcome(&r.stdout, pre), &r.stdout);
        }
        Ok(r) => {
            let stderr = r.stderr.trim();
            if stderr.contains("database is locked") {
                ctx.log.warn(
                    "autopilot fixup: ~/.copilot/data.db is locked -- close the Copilot App and \
                     re-run `dotfiles install` or `dotfiles update`, or enable the apm workflows \
                     manually from the Workflows tab",
                );
            } else if stderr.contains("no such table") {
                ctx.log.warn(
                    "autopilot fixup: the workflows table is missing from ~/.copilot/data.db; open \
                     the Copilot App once to initialize it, then re-run `dotfiles install` or \
                     `dotfiles update`",
                );
            } else if stderr.contains("no such column") {
                // Schema drift: the Copilot App database no longer matches the
                // version-2 workflows contract the embedded scripts target
                // (`id`, `mode`, `enabled`, plus the scheduling columns
                // `interval`, `schedule_hour`, `schedule_minute`, `schedule_day`
                // and `next_run_at`). Surface it loudly and name the contract so
                // the scripts can be updated, rather than letting a renamed
                // column degrade to a generic failure line.
                ctx.log.warn(&format!(
                    "autopilot fixup: ~/.copilot/data.db no longer matches the expected workflows \
                     schema (columns id, name, prompt, mode, enabled, interval, schedule_hour, \
                     schedule_minute, schedule_day, next_run_at); the Copilot App database format \
                     may have changed. Enable the apm workflows manually from the Workflows tab and \
                     report this so the dotfiles autopilot scripts can be updated: {stderr}"
                ));
            } else {
                ctx.log.warn(&format!(
                    "autopilot fixup failed (the apm operation still succeeded): {stderr}"
                ));
            }
        }
        Err(e) => {
            ctx.log.warn(&format!(
                "autopilot fixup could not run {python} (the apm operation still succeeded): {e:#}"
            ));
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

    let args = build_workflow_script_args(WORKFLOW_DESIRED_IDS_SCRIPT, &db_str, &ids);
    match ctx.executor.run_unchecked_in(&ctx.home, python, &args) {
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
