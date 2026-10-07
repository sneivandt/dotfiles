//! Embedded Python scripts and stdout parsing for workflow autopilot fixups.

use std::collections::HashSet;

/// Read-only probe of which lockfile-owned workflows still exist in the App.
pub(in crate::domains::ai::apm) const WORKFLOW_PRESENT_IDS_SCRIPT: &str =
    include_str!("../scripts/workflow_present_ids.py");

/// Build the workflow script arguments, including optional source cron JSON.
///
/// The workflow ids are passed as discrete process arguments (never shell
/// interpolated) and bound as `sqlite3` query parameters inside the script, so
/// they cannot be misinterpreted as SQL. An empty id set matches no rows,
/// including when all lockfile entries refer to removed workflows.
pub(super) fn build_workflow_script_args<'a>(
    script: &'a str,
    db: &'a str,
    ids: &'a [String],
    schedules_json: Option<&'a str>,
) -> Vec<&'a str> {
    let mut args = Vec::with_capacity(ids.len().saturating_add(5));
    args.push("-c");
    args.push(script);
    args.push(db);
    if let Some(schedules) = schedules_json {
        args.extend(["--cron-schedules", schedules]);
    }
    args.extend(ids.iter().map(String::as_str));
    args
}

/// Read-only Python stdlib `sqlite3` program that lists which of the
/// dotfiles-managed workflows are already in the desired state
/// (`mode='autopilot'`, `enabled=1`, and an armed next run when scheduled).
///
/// Invoked as `python -c <script> <db_path> <id>...` where the trailing
/// arguments are the dotfiles-managed workflow ids. They are bound as query
/// parameters in an `IN (...)` clause and the matches are printed one id per
/// line in `id` order, which [`parse_desired_ids`] reads back. On a freshly
/// reset database this prints nothing, which is the realistic pre-install
/// state.
///
/// The program lives in `scripts/workflow_desired_ids.py` and is embedded at
/// build time via [`include_str!`] so its real four-space indentation survives
/// verbatim; the repository pins `*.py` to LF endings so the embedded bytes are
/// stable across platforms.
pub(in crate::domains::ai::apm) const WORKFLOW_DESIRED_IDS_SCRIPT: &str =
    include_str!("../scripts/workflow_desired_ids.py");

/// Python stdlib `sqlite3` program that de-duplicates the dotfiles-managed
/// Copilot App workflows and flips them to autopilot.
///
/// Invoked as `python -c <script> <db_path> <id>...` where the trailing
/// arguments are the dotfiles-managed workflow ids. It first removes duplicate
/// rows for each managed id, keeping the newest row, and removes matching legacy
/// `unknown`-owner rows only for that same local package and prompt,
/// then prints two space-separated integers -- the number of those rows present
/// and the number it actually updated -- then, one per line, the id of every
/// such row now in the desired state. [`parse_autopilot_result`] reads both
/// parts back. The ids are bound as query parameters in an `IN (...)` clause so
/// the change is scoped to exactly the workflows this install deployed, and the
/// `IS NOT` comparisons are NULL-safe.
///
/// Custom cron rows are included in scheduler repair. The App represents them
/// as `interval='manual'` plus a non-empty `cron_expression`.
/// Local source metadata is passed with `--cron-schedules <json>` to restore
/// cron fields that native APM does not deploy.
///
/// The program lives in `scripts/workflow_autopilot.py` and is embedded at
/// build time via [`include_str!`] so its real four-space indentation survives
/// verbatim; the repository pins `*.py` to LF endings so the embedded bytes are
/// stable across platforms.
pub(in crate::domains::ai::apm) const WORKFLOW_AUTOPILOT_SCRIPT: &str =
    include_str!("../scripts/workflow_autopilot.py");

/// Parse the output of [`WORKFLOW_AUTOPILOT_SCRIPT`].
///
/// The first non-empty line must be exactly two parseable `u64`s
/// (`matched updated`); a third token makes the whole parse fail. Every
/// subsequent non-empty line is a post-install desired id. The `updated` count
/// is validated for shape but discarded -- the suppression decision is driven
/// by the id set diff, not the raw update count. Returns `None` when the header
/// is malformed so callers can treat it as unparseable rather than a silent
/// zero.
pub(super) fn parse_autopilot_result(stdout: &str) -> Option<(u64, HashSet<String>)> {
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let header = lines.next()?;
    let mut nums = header.split_whitespace();
    let matched = nums.next()?.parse::<u64>().ok()?;
    let _updated = nums.next()?.parse::<u64>().ok()?;
    if nums.next().is_some() {
        return None;
    }
    let ids = lines.map(ToOwned::to_owned).collect();
    Some((matched, ids))
}

/// Parse the id-per-line output of [`WORKFLOW_DESIRED_IDS_SCRIPT`].
pub(super) fn parse_desired_ids(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
