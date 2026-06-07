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

use std::collections::{BTreeSet, HashSet};
use std::io::ErrorKind;

use crate::tasks::Context;

/// Re-assert that the Copilot App workflows *this dotfiles install deployed*
/// run on autopilot.
///
/// APM installs workflow prompts into the Copilot App's `SQLite` database
/// (`~/.copilot/data.db`) secure-by-default: every row arrives
/// `mode='interactive'` and `enabled=0`, so a freshly installed automation
/// will not fire until a human re-enables it in the App's Workflows tab.  For
/// the dotfiles-managed workflows that is undesirable -- they are meant to be
/// hands-off -- so after a successful `apm install` or `apm deps update` we
/// flip exactly those rows to `mode='autopilot'` and `enabled=1`.
///
/// The set of dotfiles-managed workflow ids is read fresh from
/// `~/.apm/apm.lock.yaml` (see [`read_deployed_workflow_ids`]) -- the lockfile
/// the apm operation we just ran regenerated -- so workflows belonging to other
/// manifests are never touched.  When the lockfile records no workflows (or is
/// missing), there is nothing to do and the fixup returns quietly.
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

    let args = build_workflow_script_args(WORKFLOW_AUTOPILOT_SCRIPT, db_str, &ids);
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

/// Report the outcome of a successful autopilot-fixup script run.
///
/// Only [`FixupOutcome::Set`] produces a console line; the other outcomes are
/// steady-state or non-actionable and stay at debug level to keep idempotent
/// runs quiet.
fn report_fixup_outcome(ctx: &Context, outcome: FixupOutcome, stdout: &str) {
    match outcome {
        FixupOutcome::NoWorkflows => {
            // The lockfile listed dotfiles-managed workflows but none of them
            // are present in `~/.copilot/data.db` yet.  This is the normal
            // state until the Copilot App has run discovery for the workflows'
            // project, so keep it out of the console and just log it.
            ctx.debug_fmt(|| {
                "autopilot fixup: dotfiles-managed workflows are listed in \
                 ~/.apm/apm.lock.yaml but none were found in ~/.copilot/data.db yet"
                    .to_string()
            });
        }
        FixupOutcome::Set(n) => {
            ctx.log.always(&format!(
                "    workflows: set {n} apm workflow(s) to autopilot + enabled"
            ));
        }
        FixupOutcome::Quiet => {
            ctx.debug_fmt(|| {
                "autopilot fixup: apm workflows already autopilot + enabled (no change)".to_string()
            });
        }
        FixupOutcome::Unparsed => {
            ctx.debug_fmt(|| {
                format!(
                    "autopilot fixup: could not parse script output (continuing): {}",
                    stdout.trim()
                )
            });
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

    let args = build_workflow_script_args(WORKFLOW_DESIRED_IDS_SCRIPT, db_str, &ids);
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

/// Lockfile URI prefix under which APM records a deployed Copilot App workflow.
///
/// Each `deployed_files` entry of this shape encodes the workflow's database
/// primary key after the prefix, i.e.
/// `copilot-app-db://workflows/apm--<owner>--<pkg>--<prompt>`.
const COPILOT_APP_WORKFLOW_URI_PREFIX: &str = "copilot-app-db://workflows/";

/// Read the workflow ids this dotfiles install deployed from
/// `~/.apm/apm.lock.yaml`.
///
/// Returns `None` when the lockfile is absent or cannot be read (treated like a
/// first install / nothing-to-do), and `Some(set)` -- possibly empty -- when it
/// was parsed.  Only `deployed_files` entries under the
/// [`COPILOT_APP_WORKFLOW_URI_PREFIX`] count; agents, skills, and other
/// primitives are ignored.  Best-effort: a malformed lockfile yields an empty
/// set rather than an error so the fixup simply does nothing.
fn read_deployed_workflow_ids(ctx: &Context) -> Option<BTreeSet<String>> {
    let lock = ctx.home.join(".apm").join("apm.lock.yaml");
    let content = match std::fs::read_to_string(&lock) {
        Ok(content) => content,
        Err(e) => {
            if e.kind() != ErrorKind::NotFound {
                ctx.debug_fmt(|| {
                    format!(
                        "autopilot scope: cannot read {} (treating as no workflows): {e}",
                        lock.display()
                    )
                });
            }
            return None;
        }
    };
    Some(parse_deployed_workflow_ids(&content))
}

/// Extract the dotfiles-deployed workflow ids from APM lockfile text.
///
/// Walks `dependencies[*].deployed_files[*]` and collects every entry that
/// starts with [`COPILOT_APP_WORKFLOW_URI_PREFIX`], stripped to the bare
/// workflow id.  Any parse failure or unexpected shape yields an empty set.
fn parse_deployed_workflow_ids(lockfile: &str) -> BTreeSet<String> {
    use serde_yaml_ng::Value;

    let mut ids = BTreeSet::new();
    let Ok(value) = serde_yaml_ng::from_str::<Value>(lockfile) else {
        return ids;
    };
    let Some(deps) = value.get("dependencies").and_then(Value::as_sequence) else {
        return ids;
    };
    for dep in deps {
        let Some(files) = dep.get("deployed_files").and_then(Value::as_sequence) else {
            continue;
        };
        for file in files {
            if let Some(id) = file
                .as_str()
                .and_then(|s| s.strip_prefix(COPILOT_APP_WORKFLOW_URI_PREFIX))
                && !id.is_empty()
            {
                ids.insert(id.to_owned());
            }
        }
    }
    ids
}

/// Build the `python -c <script> <db_path> <id>...` argument vector.
///
/// The workflow ids are passed as discrete process arguments (never shell
/// interpolated) and bound as `sqlite3` query parameters inside the script, so
/// they cannot be misinterpreted as SQL.  Callers guarantee `ids` is non-empty
/// so the scripts never build an empty `IN ()` clause.
fn build_workflow_script_args<'a>(script: &'a str, db: &'a str, ids: &'a [String]) -> Vec<&'a str> {
    let mut args = Vec::with_capacity(ids.len().saturating_add(3));
    args.push("-c");
    args.push(script);
    args.push(db);
    args.extend(ids.iter().map(String::as_str));
    args
}

/// Read-only Python stdlib `sqlite3` program that lists which of the
/// dotfiles-managed workflows are already in the desired state
/// (`mode='autopilot'`, `enabled=1`).
///
/// Invoked as `python -c <script> <db_path> <id>...` where the trailing
/// arguments are the dotfiles-managed workflow ids.  They are bound as query
/// parameters in an `IN (...)` clause and the matches are printed one id per
/// line in `id` order, which [`parse_desired_ids`] reads back.  On a freshly
/// reset database this prints nothing, which is the realistic pre-install
/// state.
pub(super) const WORKFLOW_DESIRED_IDS_SCRIPT: &str = "import sqlite3,sys\n\
con=sqlite3.connect(sys.argv[1], timeout=5)\n\
con.execute(\"PRAGMA busy_timeout=5000\")\n\
ids=sys.argv[2:]\n\
ph=\",\".join(\"?\" for _ in ids)\n\
q=\"SELECT id FROM workflows WHERE id IN (\"+ph+\") AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id\"\n\
for row in con.execute(q, ids):\n\
\x20   print(row[0])\n";

/// Python stdlib `sqlite3` program that flips the dotfiles-managed Copilot App
/// workflows to autopilot.
///
/// Invoked as `python -c <script> <db_path> <id>...` where the trailing
/// arguments are the dotfiles-managed workflow ids.  It first prints two
/// space-separated integers -- the number of those rows present and the number
/// it actually updated -- then, one per line, the id of every such row now in
/// the desired state.  [`parse_autopilot_result`] reads both parts back.  The
/// ids are bound as query parameters in an `IN (...)` clause so the change is
/// scoped to exactly the workflows this install deployed, and the `IS NOT`
/// comparisons are NULL-safe.
pub(super) const WORKFLOW_AUTOPILOT_SCRIPT: &str = "import sqlite3,sys\n\
con=sqlite3.connect(sys.argv[1], timeout=5)\n\
con.execute(\"PRAGMA busy_timeout=5000\")\n\
ids=sys.argv[2:]\n\
ph=\",\".join(\"?\" for _ in ids)\n\
matched=con.execute(\"SELECT COUNT(*) FROM workflows WHERE id IN (\"+ph+\")\", ids).fetchone()[0]\n\
cur=con.execute(\"UPDATE workflows SET mode='autopilot', enabled=1 WHERE id IN (\"+ph+\") AND (mode IS NOT 'autopilot' OR enabled IS NOT 1)\", ids)\n\
con.commit()\n\
print(matched, cur.rowcount)\n\
for row in con.execute(\"SELECT id FROM workflows WHERE id IN (\"+ph+\") AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id\", ids):\n\
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixupOutcome {
    /// None of the requested dotfiles-managed workflow ids matched a row in
    /// `~/.copilot/data.db` (the lockfile listed workflows, but the App has not
    /// recorded them yet).  Logged at debug rather than warned.
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

    #[test]
    fn parse_deployed_workflow_ids_extracts_only_workflow_uris() {
        let lock = "\
lockfile_version: '1'
dependencies:
- repo_url: _local/dot-code
  deployed_files:
  - .agents/skills/project-hygiene
- repo_url: github/awesome-copilot
  deployed_files:
  - copilot-app-db://workflows/apm--awesome-copilot--planning--triage
  - .agents/skills/foo
  - copilot-app-db://workflows/apm--awesome-copilot--planning--report
- repo_url: dotnet/skills
  deployed_files:
  - copilot-app-db://workflows/apm--dotnet--diag--collect
";
        let ids = parse_deployed_workflow_ids(lock);
        let expected: BTreeSet<String> = [
            "apm--awesome-copilot--planning--triage",
            "apm--awesome-copilot--planning--report",
            "apm--dotnet--diag--collect",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert_eq!(ids, expected);
    }

    #[test]
    fn parse_deployed_workflow_ids_empty_when_no_workflows() {
        let lock = "\
dependencies:
- repo_url: _local/dot-code
  deployed_files:
  - .agents/skills/project-hygiene
";
        assert!(parse_deployed_workflow_ids(lock).is_empty());
    }

    #[test]
    fn parse_deployed_workflow_ids_empty_on_malformed_or_unrelated_yaml() {
        // A bare scalar, a mapping without `dependencies`, and a dependency
        // without `deployed_files` must all yield an empty set rather than
        // panicking or erroring.
        assert!(parse_deployed_workflow_ids("lock\n").is_empty());
        assert!(parse_deployed_workflow_ids("name: x\nversion: 1\n").is_empty());
        assert!(parse_deployed_workflow_ids("dependencies:\n- repo_url: a/b\n").is_empty());
        assert!(parse_deployed_workflow_ids(": : not yaml : :").is_empty());
    }

    #[test]
    fn parse_deployed_workflow_ids_ignores_bare_prefix() {
        // An entry that is exactly the prefix (empty id) must be dropped.
        let lock = "\
dependencies:
- repo_url: a/b
  deployed_files:
  - copilot-app-db://workflows/
";
        assert!(parse_deployed_workflow_ids(lock).is_empty());
    }

    #[test]
    fn build_workflow_script_args_appends_ids_in_order() {
        let ids = vec!["apm--a".to_string(), "apm--b".to_string()];
        let args = build_workflow_script_args(WORKFLOW_AUTOPILOT_SCRIPT, "/db", &ids);
        assert_eq!(
            args,
            ["-c", WORKFLOW_AUTOPILOT_SCRIPT, "/db", "apm--a", "apm--b"]
        );
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
    fn decide_fixup_outcome_reports_no_workflows_when_absent() {
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
