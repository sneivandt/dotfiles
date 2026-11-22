use super::*;

fn repair_changed(ctx: &Context, pre: &DesiredApmWorkflows) -> bool {
    super::super::apply_workflow_autopilot_fixup(ctx, pre)
        .expect("workflow repair must complete")
        .changed()
}

#[test]
fn cron_skips_calendar_fields_and_bounds_impossible_schedules() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let (helpers, _) = WORKFLOW_AUTOPILOT_SCRIPT
        .split_once("\ncon = sqlite3.connect")
        .unwrap();
    let loop_line = helpers
        .lines()
        .position(|line| line.trim() == "if date.month not in months:")
        .unwrap()
        + 1;
    let script = format!("{helpers}\nloop_line = {loop_line}\n");
    let script = script
        + r#"
cases = [
    ("0 0 29 2 *", datetime(2026, 3, 1), datetime(2028, 2, 29)),
    ("0 0 1 1 *", datetime(2026, 12, 31, 23, 59), datetime(2027, 1, 1)),
    ("15 7,11,15 * * 1-5", datetime(2026, 11, 13, 15, 15), datetime(2026, 11, 16, 7, 15)),
    ("0 0 31 2 1", datetime(2026, 2, 1), datetime(2026, 2, 2)),
    ("0 9 1 * */1", datetime(2026, 11, 21, 10), datetime(2026, 12, 1, 9)),
    ("0 9 */2 * MON", datetime(2026, 11, 23, 10), datetime(2026, 12, 7, 9)),
    ("* * * * *", datetime(2026, 11, 14, 12, 34, 59), datetime(2026, 11, 14, 12, 35)),
]
for expression, now, expected in cases:
    assert compute_next_cron(expression, now) == expected, expression
iterations = 0
def trace(frame, event, arg):
    global iterations
    if event == "line" and frame.f_code.co_name == "compute_next_cron" and frame.f_lineno == loop_line:
        iterations += 1
    return trace
sys.settrace(trace)
try:
    compute_next_cron("0 0 31 2 *", datetime(2026, 1, 1))
except ValueError as error:
    assert "no occurrence" in str(error)
else:
    raise AssertionError("February 31 must not be scheduled")
finally:
    sys.settrace(None)
assert iterations < 3000, iterations
"#;
    run_python_script(python, &script, &[]);
}

#[test]
fn cron_accepts_case_insensitive_names_and_preserves_numeric_rules() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let (helpers, _) = WORKFLOW_AUTOPILOT_SCRIPT
        .split_once("\ncon = sqlite3.connect")
        .unwrap();
    let script = helpers.to_owned()
        + r#"
now = datetime(2026, 11, 13, 15, 15)
cases = [
    ("15 7,11,15 * * MON-FRI", datetime(2026, 11, 16, 7, 15)),
    ("0 9 * nOv mOn,wEd,fRi", datetime(2026, 11, 16, 9)),
    ("0 9 * NOV-DEC/2 MON-FRI/2", datetime(2026, 11, 16, 9)),
    ("0 9 * JAN,MAR-MAY/2 *", datetime(2027, 1, 1, 9)),
    ("0 9 1 JAN/2 *", datetime(2027, 1, 1, 9)),
    ("0 9 * * MON/2", datetime(2026, 11, 15, 9)),
    ("0 9 * * SUN", datetime(2026, 11, 15, 9)),
    ("0 9 * * 7", datetime(2026, 11, 15, 9)),
    ("0 9 * * SUN,7", datetime(2026, 11, 15, 9)),
    ("0 9 * * FRI-7/2", datetime(2026, 11, 15, 9)),
    ("0 9 * * SUN-SAT/2", datetime(2026, 11, 14, 9)),
    ("0 9 14 NOV MON", datetime(2026, 11, 14, 9)),
    ("0 9 * 11,DEC MON,5", datetime(2026, 11, 16, 9)),
]
for expression, expected in cases:
    actual = compute_next_cron(expression, now)
    assert actual == expected, (expression, actual, expected)

for index, name in enumerate("JAN FEB MAR APR MAY JUN JUL AUG SEP OCT NOV DEC".split(), 1):
    assert compute_next_cron(f"0 0 1 {name} *", datetime(2025, 12, 31, 23, 59)) == datetime(2026, index, 1), name
for index, name in enumerate("MON TUE WED THU FRI SAT SUN".split()):
    assert compute_next_cron(f"0 9 * * {name}", datetime(2026, 11, 15, 12)) == datetime(2026, 11, 16 + index, 9), name

invalid = [
    "0 9 * FOO MON", "0 9 * JAN FOO", "0 9 * MON *", "0 9 * * JAN",
    "MON 9 * * *", "0 JAN * * *", "0 9 MON * *", "0 9 * JANUARY MONDAY",
    "0 9 * DEC-FEB *", "0 9 * * FRI-MON", "0 9 * JAN/0 MON",
    "0 9 * JAN/-1 MON", "0 9 * JAN/MAR MON", "0 9 * * MON/FRI",
    "0 9 * JAN-13 MON", "0 9 * * MON-8", "0 9 * JAN MON,,FRI",
    "0 9 * JAN MON/2/3", "@daily", "0 0 31 FEB *",
]
for expression in invalid:
    try:
        compute_next_cron(expression, now)
    except ValueError:
        pass
    else:
        raise AssertionError(f"invalid cron accepted: {expression!r}")
"#;
    run_python_script(python, &script, &[]);
}

#[test]
fn named_cron_repairs_the_workflow_batch_and_invalid_cron_rolls_it_back() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let dir = tempfile::tempdir().expect("create fixture directory");
    let db_path = dir.path().join("data.db");
    run_python_script(
        python,
        r#"
import json, sqlite3, subprocess, sys
from datetime import datetime
source, db = sys.argv[1:]
connection = sqlite3.connect(db)
connection.execute("CREATE TABLE workflows (id TEXT, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT, cron_expression TEXT)")
for wid in ["apm--daily", "apm--named", "apm--unmanaged"]:
    connection.execute("INSERT INTO workflows VALUES (?, 'Review', 'prompt', 'interactive', 0, 'daily', 9, 0, 1, NULL, NULL)", (wid,))
connection.commit()
ids = ["apm--daily", "apm--named"]
def repair(cron):
    return subprocess.run(
        [sys.executable, "-c", source, db, "--cron-schedules", json.dumps({"apm--named": cron}), *ids],
        capture_output=True, text=True,
    )
def rows():
    return connection.execute("SELECT * FROM workflows ORDER BY id, rowid").fetchall()
unmanaged = rows()[2]
result = repair("0 9 * JAN-DEC MON-FRI")
assert result.returncode == 0, result.stderr
assert result.stdout == "2 2\napm--daily\napm--named\n", result.stdout
repaired = rows()
for row in repaired[:2]:
    assert row[3:5] == ("autopilot", 1), row
    assert datetime.fromisoformat(row[9].replace("Z", "+00:00")) > datetime.now().astimezone(), row
assert repaired[1][5] == "manual", repaired
assert repaired[1][10] == "0 9 * JAN-DEC MON-FRI", repaired
named_next = datetime.fromisoformat(repaired[1][9].replace("Z", "+00:00")).astimezone()
assert named_next.hour == 9 and named_next.minute == 0 and named_next.weekday() < 5, named_next
assert repaired[2] == unmanaged, repaired
assert repair("0 9 * JAN-DEC MON-FRI").returncode == 0
assert rows() == repaired, "repeat repair must preserve next_run_at"

connection.execute("UPDATE workflows SET mode='interactive', enabled=0, next_run_at=NULL WHERE id IN (?, ?)", ids)
connection.execute("INSERT INTO workflows SELECT * FROM workflows WHERE id='apm--daily'")
connection.commit()
before_invalid = rows()
result = repair("0 9 * JAN-DEC MON-BOGUS")
assert result.returncode != 0, result.stdout
assert "ValueError" in result.stderr, result.stderr
assert not result.stdout, result.stdout
assert rows() == before_invalid, "invalid named cron must roll back deduplication, mode, source cron, and scheduler writes"
"#,
        &[
            WORKFLOW_AUTOPILOT_SCRIPT,
            db_path.to_str().expect("fixture path utf-8"),
        ],
    );
}

#[test]
fn readable_workflow_sources_use_one_process_per_snapshot_or_fixup() {
    use WorkflowStep::{Repair, Snapshot};
    let Some(fixture) = LocalCron::new() else {
        return;
    };
    write_local_cron_prompt(
        &fixture.prompt.with_file_name("removed.prompt.md"),
        Some("0 9 * * *"),
    );
    let ctx = fixture.context(false, &[Snapshot, Repair, Snapshot, Repair]);
    let initial_db = std::fs::read(&fixture.db).unwrap();
    let before = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(before, DesiredApmWorkflows::Known(id_set(&[])));
    assert_eq!(
        std::fs::read(&fixture.db).unwrap(),
        initial_db,
        "snapshot must remain read-only without the separate scope probe"
    );
    assert!(repair_changed(&ctx, &before));
    let desired = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(
        desired,
        DesiredApmWorkflows::Known(id_set(&["apm--_local--fixture--review"]))
    );
    let repaired = fixture.row();
    assert!(!repair_changed(&ctx, &desired));
    assert_eq!(
        fixture.row(),
        repaired,
        "repeat fixup must preserve schedules and leave foreign workflows unchanged"
    );
}

#[test]
fn local_source_cron_schedules_converge_with_stale_lock_entries() {
    use WorkflowStep::{InvalidRepair, Repair, Snapshot};
    let Some(fixture) = LocalCron::new() else {
        return;
    };
    let ctx = fixture.context(
        true,
        &[
            Snapshot,
            Repair,
            Snapshot,
            Repair,
            Snapshot,
            Repair,
            Snapshot,
            Snapshot,
            InvalidRepair,
            Snapshot,
            Repair,
        ],
    );
    let id = "apm--_local--fixture--review";
    let before = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(before, DesiredApmWorkflows::Known(id_set(&[])));
    assert!(repair_changed(&ctx, &before));
    let desired = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(desired, DesiredApmWorkflows::Known(id_set(&[id])));

    let initial = fixture.row();
    assert_eq!(initial[0], "autopilot");
    assert_eq!(initial[1], 1);
    assert_eq!(initial[2], "manual");
    assert_eq!(initial[3], "15 7,11,15 * * 1-5");
    assert!(initial[4].is_string(), "{initial}");
    assert!(!repair_changed(&ctx, &desired));
    assert_eq!(
        fixture.row(),
        initial,
        "repeat fixup must preserve next_run_at"
    );

    write_local_cron_prompt(&fixture.prompt, Some("45 18 * * 1-5"));
    let before_change = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(before_change, DesiredApmWorkflows::Known(id_set(&[])));
    assert!(repair_changed(&ctx, &before_change));
    assert_eq!(
        super::super::snapshot_desired_apm_workflow_ids(&ctx),
        DesiredApmWorkflows::Known(id_set(&[id]))
    );
    let changed = fixture.row();
    assert_eq!(changed[3], "45 18 * * 1-5");
    assert_ne!(
        changed[4], initial[4],
        "changed cron must invalidate the old next run"
    );

    write_local_cron_prompt(&fixture.prompt, Some("99 18 * * 1-5"));
    let before_invalid = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert!(super::super::apply_workflow_autopilot_fixup(&ctx, &before_invalid).is_err());
    assert_eq!(
        fixture.row(),
        changed,
        "invalid cron must roll back all writes"
    );

    write_local_cron_prompt(&fixture.prompt, None);
    let before_removal = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(before_removal, DesiredApmWorkflows::Known(id_set(&[])));
    assert!(repair_changed(&ctx, &before_removal));
    let removed = fixture.row();
    assert!(removed[3].is_null(), "{removed}");
    assert!(removed[4].is_null(), "{removed}");
}

#[test]
fn missing_active_workflow_source_is_reported_without_mutation() {
    let Some(fixture) = LocalCron::new() else {
        return;
    };
    std::fs::remove_file(&fixture.prompt).unwrap();
    let initial = fixture.row();
    let ctx = fixture.context(false, &[WorkflowStep::Scope; 2]);
    let pre = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(pre, DesiredApmWorkflows::Unavailable);
    assert!(super::super::apply_workflow_autopilot_fixup(&ctx, &pre).is_err());
    assert_eq!(fixture.row(), initial);
}

#[test]
fn removed_workflow_sources_are_not_read_when_no_managed_rows_remain() {
    let Some(fixture) = LocalCron::new() else {
        return;
    };
    std::fs::remove_dir_all(fixture.prompt.parent().unwrap()).unwrap();
    run_python_script(
        fixture.python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute("DELETE FROM workflows WHERE id='apm--_local--fixture--review'")
con.commit()
"#,
        &[fixture.db.to_str().unwrap()],
    );

    let before = std::fs::read(&fixture.db).unwrap();
    let ctx = fixture.context(true, &[WorkflowStep::Snapshot, WorkflowStep::Repair]);
    let pre = super::super::snapshot_desired_apm_workflow_ids(&ctx);
    assert_eq!(pre, DesiredApmWorkflows::Known(id_set(&[])));
    assert!(!repair_changed(&ctx, &pre));
    assert_eq!(std::fs::read(&fixture.db).unwrap(), before);
}

#[test]
fn stale_workflow_cron_metadata_does_not_require_new_database_schema() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    run_python_script(
        python,
        r#"
import contextlib, io, json, sqlite3, sys
from unittest.mock import patch
source = sys.argv[1]
connection = sqlite3.connect(":memory:")
connection.execute("CREATE TABLE workflows (id TEXT, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT)")
connection.execute("INSERT INTO workflows VALUES ('apm--fixture', 'Review', 'prompt', 'interactive', 0, 'manual', 9, 0, 1, NULL)")
connection.commit()
with (
    patch("sqlite3.connect", return_value=connection),
    patch.object(sys, "argv", ["fixup", ":memory:", "--cron-schedules", json.dumps({"apm--removed": "0 9 * * *"}), "apm--fixture", "apm--removed"]),
    contextlib.redirect_stdout(io.StringIO()) as output,
):
    exec(compile(source, "workflow_autopilot.py", "exec"), {})
assert output.getvalue() == "1 1\napm--fixture\n", output.getvalue()
assert connection.execute("SELECT id, mode, enabled, next_run_at FROM workflows").fetchall() == [("apm--fixture", "autopilot", 1, None)]
"#,
        &[WORKFLOW_AUTOPILOT_SCRIPT],
    );
}

#[test]
fn workflow_source_cron_requires_schema_support_without_mutation() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    run_python_script(
        python,
        r#"
import contextlib, io, json, sqlite3, sys
from unittest.mock import patch
source = sys.argv[1]
connection = sqlite3.connect(":memory:")
connection.execute("CREATE TABLE workflows (id TEXT, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT)")
connection.execute("INSERT INTO workflows VALUES ('apm--fixture', 'Review', 'prompt', 'interactive', 0, 'manual', 9, 0, 1, NULL)")
connection.commit()
with (
    patch("sqlite3.connect", return_value=connection),
    patch.object(sys, "argv", ["fixup", ":memory:", "--cron-schedules", json.dumps({"apm--fixture": "0 9 * * *"}), "apm--fixture"]),
    contextlib.redirect_stdout(io.StringIO()),
):
    try:
        exec(compile(source, "workflow_autopilot.py", "exec"), {})
    except ValueError as error:
        assert "no cron_expression column" in str(error), error
    else:
        raise AssertionError("missing cron schema must fail explicitly")
assert connection.execute("SELECT mode, enabled, next_run_at FROM workflows").fetchone() == ("interactive", 0, None)
"#,
        &[WORKFLOW_AUTOPILOT_SCRIPT],
    );
}

/// Regression guard: the embedded Python scripts must keep the `print`
/// body indented under its `for` loop. Rust string `\`-continuations strip
/// the leading whitespace of the next source line, which previously
/// flattened the indent and produced an `IndentationError` at real install
/// time (dry-run never executes these scripts, so only a live install hit
/// it). Assert the runtime bytes carry a four-space indented `print`.
#[test]
fn workflow_scripts_keep_python_indentation() {
    for script in [WORKFLOW_DESIRED_IDS_SCRIPT, WORKFLOW_AUTOPILOT_SCRIPT] {
        assert!(
            script.contains("\n    print(row[0])\n"),
            "script must indent the for-loop body by four spaces:\n{script}"
        );
        assert!(
            !script.contains("):\nprint("),
            "script must not flatten the for-loop body indentation:\n{script}"
        );
    }
}
