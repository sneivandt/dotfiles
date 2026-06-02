//! Copilot App workflow autopilot fixup for APM-managed workflows.
//!
//! After `apm install` rewrites workflow rows secure-by-default, this module
//! re-asserts the `apm--*` workflows to autopilot + enabled and decides, via a
//! ground-truth pre/post snapshot, whether anything actually changed so that
//! steady-state runs stay quiet.

use std::collections::HashSet;

use crate::tasks::Context;

/// Re-assert that APM-managed Copilot App workflows run on autopilot.
///
/// APM installs workflow prompts into the Copilot App's `SQLite` database
/// (`~/.copilot/data.db`) secure-by-default: every row arrives
/// `mode='interactive'` and `enabled=0`, so a freshly installed automation
/// will not fire until a human re-enables it in the App's Workflows tab.  For
/// the small set of workflows this repo deploys that is undesirable -- they are
/// meant to be hands-off -- so after a successful `apm install` we flip the
/// `apm--*` rows to `mode='autopilot'` and `enabled=1`.
///
/// This is strictly best-effort and never fails the task: APM has already done
/// the real work by the time we get here.  The most common failure is a locked
/// database, which means the Copilot App is currently open and holding the
/// lock; we surface that loudly so the user knows to close the App (or just
/// toggle the workflows by hand).  The update runs through Python's stdlib
/// `sqlite3` module so we do not need a `SQLite` binary on PATH or a Rust
/// `SQLite` dependency.
pub(super) fn apply_workflow_autopilot_fixup(ctx: &Context, pre: &DesiredApmWorkflows) {
    let db = ctx.home.join(".copilot").join("data.db");
    match db.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            ctx.debug_fmt(|| format!("skipping autopilot fixup: {} does not exist", db.display()));
            return;
        }
        Err(e) => {
            ctx.debug_fmt(|| {
                format!(
                    "skipping autopilot fixup: cannot stat {}: {e}",
                    db.display()
                )
            });
            return;
        }
    }

    let Some(db_str) = db.to_str() else {
        ctx.log.warn(&format!(
            "skipping autopilot fixup: database path {} is not valid UTF-8",
            db.display()
        ));
        return;
    };

    let python = if ctx.executor.which("python3") {
        "python3"
    } else if ctx.executor.which("python") {
        "python"
    } else {
        ctx.log.warn(
            "skipping autopilot fixup: neither python3 nor python found in PATH; enable the apm \
             workflows manually from the Copilot App's Workflows tab",
        );
        return;
    };

    match ctx.executor.run_unchecked_in(
        &ctx.home,
        python,
        &["-c", WORKFLOW_AUTOPILOT_SCRIPT, db_str],
    ) {
        Ok(r) if r.success => match decide_fixup_outcome(&r.stdout, pre) {
            FixupOutcome::NoWorkflows => {
                ctx.log.warn(
                    "autopilot fixup: no apm-managed workflows found in ~/.copilot/data.db \
                     (expected the apm--* rows from this install)",
                );
            }
            FixupOutcome::Set(n) => {
                ctx.log.always(&format!(
                    "    workflows: set {n} apm workflow(s) to autopilot + enabled"
                ));
            }
            FixupOutcome::Quiet => {
                ctx.debug_fmt(|| {
                    "autopilot fixup: apm workflows already autopilot + enabled (no change)"
                        .to_string()
                });
            }
            FixupOutcome::Unparsed => {
                ctx.debug_fmt(|| {
                    format!(
                        "autopilot fixup: could not parse script output (continuing): {}",
                        r.stdout.trim()
                    )
                });
            }
        },
        Ok(r) => {
            let stderr = r.stderr.trim();
            if stderr.contains("database is locked") {
                ctx.log.warn(
                    "autopilot fixup: ~/.copilot/data.db is locked -- close the Copilot App and \
                     re-run `dotfiles install`, or enable the apm workflows manually from the \
                     Workflows tab",
                );
            } else if stderr.contains("no such table") {
                ctx.log.warn(
                    "autopilot fixup: the workflows table is missing from ~/.copilot/data.db; open \
                     the Copilot App once to initialize it, then re-run `dotfiles install`",
                );
            } else {
                ctx.log.warn(&format!(
                    "autopilot fixup failed (apm install still succeeded): {stderr}"
                ));
            }
        }
        Err(e) => {
            ctx.log.warn(&format!(
                "autopilot fixup could not run {python} (apm install still succeeded): {e:#}"
            ));
        }
    }
}

/// Ground-truth snapshot of which `apm--*` workflows were already in the desired
/// state (`mode='autopilot'`, `enabled=1`) before `apm install` mutated the
/// Copilot App database.
///
/// Captured up front so the post-install fixup can report a real delta instead
/// of the full set APM resets secure-by-default on every run.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DesiredApmWorkflows {
    /// The pre-install desired ids were read successfully (possibly empty).
    Known(HashSet<String>),
    /// No `~/.copilot/data.db` (or no `workflows` table) yet -- a first install,
    /// where every workflow the fixup ends up setting is a genuine change.
    FirstInstall,
    /// The snapshot could not be taken (no Python, locked db, bad UTF-8, ...).
    /// The fixup stays quiet to avoid reporting a change it cannot substantiate.
    Unavailable,
}

/// Read the set of already-desired `apm--*` workflow ids before install.
///
/// Best-effort and read-only: every failure path returns a non-`Known` variant
/// and logs at debug level, never warning, because a missing snapshot must not
/// produce a false "set N workflow(s)" line later.
pub(super) fn snapshot_desired_apm_workflow_ids(ctx: &Context) -> DesiredApmWorkflows {
    let db = ctx.home.join(".copilot").join("data.db");
    match db.try_exists() {
        Ok(true) => {}
        Ok(false) => return DesiredApmWorkflows::FirstInstall,
        Err(e) => {
            ctx.debug_fmt(|| format!("apm workflow snapshot: cannot stat {}: {e}", db.display()));
            return DesiredApmWorkflows::Unavailable;
        }
    }

    let Some(db_str) = db.to_str() else {
        ctx.debug_fmt(|| {
            format!(
                "apm workflow snapshot: database path {} is not valid UTF-8",
                db.display()
            )
        });
        return DesiredApmWorkflows::Unavailable;
    };

    let python = if ctx.executor.which("python3") {
        "python3"
    } else if ctx.executor.which("python") {
        "python"
    } else {
        ctx.debug_fmt(|| {
            "apm workflow snapshot: neither python3 nor python found in PATH".to_string()
        });
        return DesiredApmWorkflows::Unavailable;
    };

    match ctx.executor.run_unchecked_in(
        &ctx.home,
        python,
        &["-c", WORKFLOW_DESIRED_IDS_SCRIPT, db_str],
    ) {
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

/// Read-only Python stdlib `sqlite3` program that lists the `apm--*` workflows
/// already in the desired state (`mode='autopilot'`, `enabled=1`).
///
/// Invoked as `python -c <script> <db_path>`.  Prints one id per line in `id`
/// order, which [`parse_desired_ids`] reads back.  On a freshly reset database
/// this prints nothing, which is the realistic pre-install state.
pub(super) const WORKFLOW_DESIRED_IDS_SCRIPT: &str = "import sqlite3,sys\n\
con=sqlite3.connect(sys.argv[1], timeout=5)\n\
con.execute(\"PRAGMA busy_timeout=5000\")\n\
for row in con.execute(\"SELECT id FROM workflows WHERE id GLOB 'apm--*' AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id\"):\n\
\x20   print(row[0])\n";

/// Python stdlib `sqlite3` program that flips APM-managed Copilot App workflows
/// to autopilot.
///
/// Invoked as `python -c <script> <db_path>`.  It first prints two
/// space-separated integers -- the number of `apm--*` rows present and the
/// number it actually updated -- then, one per line, the id of every `apm--*`
/// row now in the desired state.  [`parse_autopilot_result`] reads both parts
/// back.  The `IS NOT` comparisons are NULL-safe and the `id GLOB 'apm--*'`
/// filter scopes the change to APM-owned rows so hand-authored workflows are
/// never touched.
pub(super) const WORKFLOW_AUTOPILOT_SCRIPT: &str = "import sqlite3,sys\n\
con=sqlite3.connect(sys.argv[1], timeout=5)\n\
con.execute(\"PRAGMA busy_timeout=5000\")\n\
matched=con.execute(\"SELECT COUNT(*) FROM workflows WHERE id GLOB 'apm--*'\").fetchone()[0]\n\
cur=con.execute(\"UPDATE workflows SET mode='autopilot', enabled=1 WHERE id GLOB 'apm--*' AND (mode IS NOT 'autopilot' OR enabled IS NOT 1)\")\n\
con.commit()\n\
print(matched, cur.rowcount)\n\
for row in con.execute(\"SELECT id FROM workflows WHERE id GLOB 'apm--*' AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id\"):\n\
\x20   print(row[0])\n";

/// Parse the output of [`WORKFLOW_AUTOPILOT_SCRIPT`].
///
/// The first non-empty line must be exactly two parseable `u64`s
/// (`matched updated`); a third token makes the whole parse fail.  Every
/// subsequent non-empty line is a post-install desired id.  The `updated`
/// count is validated for shape but discarded -- the suppression decision is
/// driven by the id set diff, not the raw update count.  Returns `None` when
/// the header is malformed so callers can treat it as unparseable rather than a
/// silent zero.
fn parse_autopilot_result(stdout: &str) -> Option<(u64, HashSet<String>)> {
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
fn parse_desired_ids(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// What the post-install autopilot fixup should report, derived purely from the
/// script output and the pre-install snapshot.
#[derive(Debug, PartialEq, Eq)]
enum FixupOutcome {
    /// No `apm--*` rows were present at all -- warn (the install should have
    /// created them).
    NoWorkflows,
    /// `n` workflows newly reached the desired state; announce it.
    Set(usize),
    /// Nothing meaningfully changed (or the change cannot be substantiated);
    /// stay quiet.
    Quiet,
    /// The script output could not be parsed; log at debug and continue.
    Unparsed,
}

/// Decide what the fixup should report by diffing the post-install desired set
/// against the pre-install snapshot.
///
/// This is the suppression core: in the steady state APM resets every workflow
/// secure-by-default and the fixup restores them, so the post set equals the
/// pre set and the delta is zero -- [`FixupOutcome::Quiet`].  A non-zero delta
/// means a workflow genuinely transitioned (first install, a newly added
/// workflow, or a user-disabled one being re-enabled).
fn decide_fixup_outcome(stdout: &str, pre: &DesiredApmWorkflows) -> FixupOutcome {
    let Some((matched, post_ids)) = parse_autopilot_result(stdout) else {
        return FixupOutcome::Unparsed;
    };
    if matched == 0 {
        return FixupOutcome::NoWorkflows;
    }
    let delta = match pre {
        DesiredApmWorkflows::Known(pre_ids) => post_ids.difference(pre_ids).count(),
        DesiredApmWorkflows::FirstInstall => post_ids.len(),
        DesiredApmWorkflows::Unavailable => return FixupOutcome::Quiet,
    };
    if delta > 0 {
        FixupOutcome::Set(delta)
    } else {
        FixupOutcome::Quiet
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code uses panicking helpers"
)]
mod tests {
    use super::*;

    fn id_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_autopilot_result_reads_header_and_ids() {
        let (matched, ids) =
            parse_autopilot_result("3 3\napm--a\napm--b\napm--c\n").expect("valid output parses");
        assert_eq!(matched, 3);
        assert_eq!(ids, id_set(&["apm--a", "apm--b", "apm--c"]));
    }

    #[test]
    fn parse_autopilot_result_allows_zero_ids() {
        let (matched, ids) = parse_autopilot_result("0 0\n").expect("header-only output parses");
        assert_eq!(matched, 0);
        assert!(ids.is_empty());
    }

    #[test]
    fn parse_autopilot_result_rejects_three_token_header() {
        assert!(parse_autopilot_result("3 3 oops\napm--a\n").is_none());
    }

    #[test]
    fn parse_autopilot_result_rejects_empty_output() {
        assert!(parse_autopilot_result("").is_none());
        assert!(parse_autopilot_result("\n\n").is_none());
    }

    #[test]
    fn parse_desired_ids_filters_blank_lines() {
        let ids = parse_desired_ids("apm--a\n\n  apm--b  \n\n");
        assert_eq!(ids, id_set(&["apm--a", "apm--b"]));
    }

    #[test]
    fn parse_desired_ids_empty_is_empty_set() {
        assert!(parse_desired_ids("").is_empty());
    }

    /// Regression guard: the embedded Python scripts must keep the `print`
    /// body indented under its `for` loop.  Rust string `\`-continuations strip
    /// the leading whitespace of the next source line, which previously
    /// flattened the indent and produced an `IndentationError` at real install
    /// time (dry-run never executes these scripts, so only a live install hit
    /// it).  Assert the runtime bytes carry a four-space indented `print`.
    #[test]
    fn workflow_scripts_keep_python_indentation() {
        for script in [WORKFLOW_DESIRED_IDS_SCRIPT, WORKFLOW_AUTOPILOT_SCRIPT] {
            assert!(
                script.contains("):\n    print(row[0])\n"),
                "script must indent the for-loop body by four spaces:\n{script}"
            );
            assert!(
                !script.contains("):\nprint("),
                "script must not flatten the for-loop body indentation:\n{script}"
            );
        }
    }

    #[test]
    fn decide_fixup_outcome_quiet_in_steady_state() {
        // Pre-install the three workflows are already desired; the install
        // resets them and the fixup restores the same three -- no net change.
        let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"]));
        let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
        assert_eq!(outcome, FixupOutcome::Quiet);
    }

    #[test]
    fn decide_fixup_outcome_set_all_on_first_install() {
        let pre = DesiredApmWorkflows::FirstInstall;
        let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
        assert_eq!(outcome, FixupOutcome::Set(3));
    }

    #[test]
    fn decide_fixup_outcome_set_one_when_workflow_added() {
        let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b"]));
        let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
        assert_eq!(outcome, FixupOutcome::Set(1));
    }

    #[test]
    fn decide_fixup_outcome_set_one_when_user_disabled_then_reenabled() {
        // apm--b was disabled by the user pre-install; the fixup re-enables it.
        let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--c"]));
        let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
        assert_eq!(outcome, FixupOutcome::Set(1));
    }

    #[test]
    fn decide_fixup_outcome_quiet_when_workflow_removed() {
        // A workflow desired pre-install is gone post-install: the post set is a
        // subset of pre, so the forward diff is zero and nothing was set.
        let pre = DesiredApmWorkflows::Known(id_set(&["apm--a", "apm--b", "apm--c"]));
        let outcome = decide_fixup_outcome("2 2\napm--a\napm--b\n", &pre);
        assert_eq!(outcome, FixupOutcome::Quiet);
    }

    #[test]
    fn decide_fixup_outcome_warns_when_no_workflows() {
        let pre = DesiredApmWorkflows::FirstInstall;
        let outcome = decide_fixup_outcome("0 0\n", &pre);
        assert_eq!(outcome, FixupOutcome::NoWorkflows);
    }

    #[test]
    fn decide_fixup_outcome_quiet_when_snapshot_unavailable() {
        // Without a trustworthy pre-install snapshot we cannot prove a change,
        // so stay quiet rather than emit a spurious "set N" line.
        let pre = DesiredApmWorkflows::Unavailable;
        let outcome = decide_fixup_outcome("3 3\napm--a\napm--b\napm--c\n", &pre);
        assert_eq!(outcome, FixupOutcome::Quiet);
    }

    #[test]
    fn decide_fixup_outcome_unparsed_on_malformed_output() {
        let pre = DesiredApmWorkflows::FirstInstall;
        assert_eq!(
            decide_fixup_outcome("not-a-number\n", &pre),
            FixupOutcome::Unparsed
        );
        assert_eq!(
            decide_fixup_outcome("3 3 extra\napm--a\n", &pre),
            FixupOutcome::Unparsed
        );
    }
}
