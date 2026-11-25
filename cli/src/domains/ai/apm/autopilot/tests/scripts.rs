use super::*;

#[test]
fn workflow_scope_probe_is_read_only_and_scoped() {
    let Some(fixture) = LocalCron::new() else {
        return;
    };
    let before = std::fs::read(&fixture.db).unwrap();
    for (ids, expected) in [
        (
            vec![
                "apm--_local--fixture--review",
                "apm--_local--fixture--removed",
                "' OR 1=1 --",
            ],
            "apm--_local--fixture--review",
        ),
        (vec!["apm--_local--fixture--removed"], ""),
    ] {
        let mut args = vec![fixture.db.to_str().unwrap()];
        args.extend(ids);
        let output = run_python_script(fixture.python, WORKFLOW_PRESENT_IDS_SCRIPT, &args);

        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
        assert_eq!(std::fs::read(&fixture.db).unwrap(), before);
    }
    let missing = fixture.home.path().join("missing.db");
    for script in [
        WORKFLOW_PRESENT_IDS_SCRIPT,
        WORKFLOW_DESIRED_IDS_SCRIPT,
        WORKFLOW_AUTOPILOT_SCRIPT,
    ] {
        let output = run_python_script_unchecked(
            fixture.python,
            script,
            &[missing.to_str().unwrap(), "apm--fixture"],
        );
        assert!(!output.status.success());
        assert!(
            !missing.exists(),
            "workflow helpers must not recreate a removed database"
        );
    }
}

#[test]
fn workflow_autopilot_script_uses_scheduled_date_timezone() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    run_python_script(
        python,
        r#"
import contextlib, io, sqlite3, struct, sys
from datetime import datetime, timezone
from unittest.mock import patch
from zoneinfo import ZoneInfo

# A self-contained TZif fixture: US Pacific transitions in 2026. No system
# zoneinfo database, tzdata package, process TZ mutation, or time.tzset needed.
transitions = [
    int(datetime(2026, 3, 8, 10, tzinfo=timezone.utc).timestamp()),
    int(datetime(2026, 11, 1, 9, tzinfo=timezone.utc).timestamp()),
]
tzif = (
    b"TZif\0" + b"\0" * 15 + struct.pack(">6l", 0, 0, 0, 2, 2, 8)
    + struct.pack(">2l", *transitions) + bytes([1, 0])
    + struct.pack(">lbb", -8 * 3600, 0, 0)
    + struct.pack(">lbb", -7 * 3600, 1, 4)
    + b"PST\0PDT\0"
)
local_zone = ZoneInfo.from_file(io.BytesIO(tzif))

class LocalDateTime(datetime):
    current = None

    @classmethod
    def now(cls, tz=None):
        value = cls(*cls.current)
        return value if tz is None else value.astimezone(tz)

    def astimezone(self, tz=None):
        value = self if self.tzinfo is not None else self.replace(tzinfo=local_zone)
        result = datetime.astimezone(value, local_zone if tz is None else tz)
        # Python's no-argument astimezone returns a fixed-offset local timezone.
        return result.replace(tzinfo=timezone(result.utcoffset())) if tz is None else result

source = sys.argv[1]
for name, current, expected in [
    ("spring", (2026, 3, 7, 9, 1), "2026-03-08T16:00:00.000Z"),
    ("fall", (2026, 10, 31, 9, 1), "2026-11-01T17:00:00.000Z"),
]:
    LocalDateTime.current = current
    assert type(LocalDateTime.now().astimezone().tzinfo) is timezone
    connection = sqlite3.connect(":memory:")
    connection.execute(
        "CREATE TABLE workflows (id TEXT PRIMARY KEY, name TEXT, prompt TEXT, "
        "mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, "
        "schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT, cron_expression TEXT)"
    )
    rows = [
        ("apm--daily", "daily", None, None),
        ("apm--weekly", "weekly", None, None),
        ("apm--cron", "manual", "0 9 * * *", None),
        ("apm--manual", "manual", None, None),
        ("apm--future", "daily", None, "2099-01-01T00:00:00.000Z"),
    ]
    for wid, interval, cron, next_run in rows:
        connection.execute(
            "INSERT INTO workflows VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (wid, wid, "prompt", "interactive", 0, interval, 9, 0, 0, next_run, cron),
        )
    connection.commit()
    with (
        patch("datetime.datetime", LocalDateTime),
        patch("sqlite3.connect", return_value=connection),
        patch.object(sys, "argv", ["workflow_autopilot", ":memory:"] + [row[0] for row in rows]),
        contextlib.redirect_stdout(io.StringIO()),
    ):
        exec(compile(source, "workflow_autopilot.py", "exec"), {})
    for wid in ["apm--daily", "apm--weekly", "apm--cron"]:
        actual = connection.execute(
            "SELECT mode, enabled, next_run_at FROM workflows WHERE id=?", (wid,)
        ).fetchone()
        assert actual == ("autopilot", 1, expected), (name, wid, actual, expected)
    assert connection.execute(
        "SELECT next_run_at FROM workflows WHERE id='apm--manual'"
    ).fetchone()[0] is None
    assert connection.execute(
        "SELECT next_run_at FROM workflows WHERE id='apm--future'"
    ).fetchone()[0] == "2099-01-01T00:00:00.000Z"
    connection.close()
helpers = source.split("\ncon = sqlite3.connect", 1)[0]
scope = {}
with patch("datetime.datetime", LocalDateTime):
    exec(compile(helpers, "workflow_autopilot.py", "exec"), scope)
for wall_time, expected in [
    (LocalDateTime(2026, 11, 1, 1, 15, fold=0), "2026-11-01T08:30:00.000Z"),
    (LocalDateTime(2026, 11, 1, 1, 45, fold=0), "2026-11-01T09:30:00.000Z"),
    (LocalDateTime(2026, 11, 1, 1, 15, fold=1), "2026-11-01T09:30:00.000Z"),
]:
    actual = scope["compute_next_run"]("manual", "30 1 * * *", 0, 0, 0, wall_time)
    assert actual == expected, (wall_time, wall_time.fold, actual, expected)
assert scope["compute_next_run"](
    "manual", "30 2 * * *", 0, 0, 0, LocalDateTime(2026, 3, 8, 1, 45)
) == "2026-03-09T09:30:00.000Z"
print("spring and fall schedule repairs passed")
"#,
        &[WORKFLOW_AUTOPILOT_SCRIPT],
    );
}

#[test]
fn workflow_autopilot_script_parses_utc_timestamps() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    run_python_script(
        python,
        r#"
import sys
from datetime import datetime, timezone

helpers = sys.argv[1].split("\ncon = sqlite3.connect", 1)[0]
scope = {}
exec(compile(helpers, "workflow_autopilot.py", "exec"), scope)
parse_utc = scope["parse_utc"]

assert parse_utc("2026-04-12T10:00:00.750Z") == datetime(
    2026, 4, 12, 10, 0, 0, 750000, tzinfo=timezone.utc
)
assert parse_utc("2026-04-12T10:00:00Z") == datetime(
    2026, 4, 12, 10, 0, 0, tzinfo=timezone.utc
)
assert parse_utc("2026-04-12T12:00:00+02:00") == datetime(
    2026, 4, 12, 10, 0, 0, tzinfo=timezone.utc
)
assert parse_utc("2026-04-12T10:00:00.garbageZ") is None
assert parse_utc("2026-04-12T10:00:00") is None
assert parse_utc(None) is None
"#,
        &[WORKFLOW_AUTOPILOT_SCRIPT],
    );
}

#[test]
fn workflow_autopilot_script_deduplicates_managed_rows() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("data.db");
    let db = db_path.to_str().expect("db path utf-8");
    run_python_script(
        python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute("CREATE TABLE workflows (id TEXT, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT)")
con.executemany(
    "INSERT INTO workflows VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    [
        ("apm--unknown--fixture--triage", "PR Triage", "prompt-a", "autopilot", 1, "hourly", 9, 0, 1, "2099-01-01T00:00:00.000Z"),
        ("apm--_local--fixture--triage", "PR Triage", "prompt-a", "autopilot", 1, "hourly", 9, 0, 1, "2099-01-01T00:00:00.000Z"),
        ("apm--_local--fixture--triage", "PR Triage", "prompt-a", "interactive", 0, "hourly", 9, 0, 1, None),
        ("apm--b", "PR Review", "prompt-b", "interactive", 0, "daily", 9, 0, 1, None),
        ("personal-copy", "PR Triage", "prompt-a", "interactive", 0, "hourly", 9, 0, 1, None),
        ("apm--foreign--fixture--triage", "PR Triage", "prompt-a", "interactive", 0, "hourly", 9, 0, 1, None),
        ("apm--unknown--other--triage", "PR Triage", "prompt-a", "interactive", 0, "hourly", 9, 0, 1, None),
        ("foreign--workflow", "Foreign", "prompt-foreign", "interactive", 0, "hourly", 9, 0, 1, None),
        ("foreign--workflow", "Foreign", "prompt-foreign", "interactive", 0, "hourly", 9, 0, 1, None),
    ],
)
con.commit()
"#,
        &[db],
    );

    let fixup = run_python_script(
        python,
        WORKFLOW_AUTOPILOT_SCRIPT,
        &[db, "apm--_local--fixture--triage", "apm--b"],
    );

    assert_eq!(
        String::from_utf8(fixup.stdout)
            .expect("stdout utf-8")
            .replace("\r\n", "\n"),
        "3 2\napm--_local--fixture--triage\napm--b\n"
    );

    let query = run_python_script(
        python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
for row in con.execute("SELECT id, COUNT(*), MIN(mode), MIN(enabled) FROM workflows GROUP BY id ORDER BY id"):
    print("|".join(map(str, row)))
"#,
        &[db],
    );

    assert_eq!(
        String::from_utf8(query.stdout)
            .expect("stdout utf-8")
            .replace("\r\n", "\n"),
        "apm--_local--fixture--triage|1|autopilot|1\napm--b|1|autopilot|1\n\
         apm--foreign--fixture--triage|1|interactive|0\n\
         apm--unknown--other--triage|1|interactive|0\n\
         foreign--workflow|2|interactive|0\npersonal-copy|1|interactive|0\n"
    );
}

#[test]
fn workflow_deduplication_preserves_distinct_ids_and_cron_schedules() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("data.db");
    let db = db_path.to_str().expect("db path utf-8");
    run_python_script(
        python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute("CREATE TABLE workflows (id TEXT PRIMARY KEY, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT, cron_expression TEXT)")
rows = [
    ("apm--_local--fixture--morning", "0 9 * * 1-5"),
    ("apm--_local--fixture--afternoon", "0 15 * * 1-5"),
    ("apm--_local--fixture--same-schedule", "0 9 * * 1-5"),
    ("apm--unknown--fixture--morning", "0 15 * * 1-5"),
    ("apm--unknown--fixture--same-schedule", "0 9 * * 1-5"),
]
for wid, cron in rows:
    con.execute("INSERT INTO workflows VALUES (?, 'Review', 'prompt', 'autopilot', 1, 'manual', 9, 0, 1, '2099-01-01T00:00:00.000Z', ?)", (wid, cron))
con.commit()
"#,
        &[db],
    );

    let args = [
        db,
        "apm--_local--fixture--morning",
        "apm--_local--fixture--afternoon",
        "apm--_local--fixture--same-schedule",
        "apm--unknown--fixture--same-schedule",
    ];
    for _ in 0..2 {
        run_python_script(python, WORKFLOW_AUTOPILOT_SCRIPT, &args);

        let query = run_python_script(
            python,
            r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
for wid, cron in con.execute("SELECT id, cron_expression FROM workflows ORDER BY id"):
    print(wid + "|" + cron)
"#,
            &[db],
        );

        assert_eq!(
            String::from_utf8_lossy(&query.stdout).replace("\r\n", "\n"),
            "apm--_local--fixture--afternoon|0 15 * * 1-5\n\
             apm--_local--fixture--morning|0 9 * * 1-5\n\
             apm--_local--fixture--same-schedule|0 9 * * 1-5\n\
             apm--unknown--fixture--morning|0 15 * * 1-5\n\
             apm--unknown--fixture--same-schedule|0 9 * * 1-5\n"
        );
    }
}

#[test]
fn workflow_scripts_restore_custom_cron_schedule() {
    let Some(python) = python_for_script_tests() else {
        return;
    };
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("data.db");
    let db = db_path.to_str().expect("db path utf-8");
    run_python_script(
        python,
        r#"
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute("CREATE TABLE workflows (id TEXT, name TEXT, prompt TEXT, mode TEXT, enabled INTEGER, interval TEXT, schedule_hour INTEGER, schedule_minute INTEGER, schedule_day INTEGER, next_run_at TEXT, cron_expression TEXT)")
con.execute(
    "INSERT INTO workflows VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    ("apm--cron", "PR Review", "prompt", "autopilot", 1, "manual", 9, 0, 1, None, "0 9,11,13,15 * * 1-5"),
)
con.commit()
"#,
        &[db],
    );

    let before = run_python_script(python, WORKFLOW_DESIRED_IDS_SCRIPT, &[db, "apm--cron"]);

    assert!(
        before.stdout.is_empty(),
        "an unarmed cron workflow must not be desired"
    );

    let fixup = run_python_script(python, WORKFLOW_AUTOPILOT_SCRIPT, &[db, "apm--cron"]);

    assert_eq!(
        String::from_utf8(fixup.stdout)
            .expect("stdout utf-8")
            .replace("\r\n", "\n"),
        "1 0\napm--cron\n"
    );

    let query = run_python_script(
        python,
        r#"
import sqlite3, sys
from datetime import datetime
con = sqlite3.connect(sys.argv[1])
next_run_at = con.execute("SELECT next_run_at FROM workflows WHERE id='apm--cron'").fetchone()[0]
next_local = datetime.fromisoformat(next_run_at.replace("Z", "+00:00")).astimezone()
print(next_local > datetime.now().astimezone())
print(next_local.minute == 0)
print(next_local.hour in {9, 11, 13, 15})
print(next_local.isoweekday() <= 5)
"#,
        &[db],
    );

    assert_eq!(
        String::from_utf8_lossy(&query.stdout)
            .replace("\r\n", "\n")
            .trim(),
        "True\nTrue\nTrue\nTrue"
    );

    let after = run_python_script(python, WORKFLOW_DESIRED_IDS_SCRIPT, &[db, "apm--cron"]);

    assert_eq!(
        String::from_utf8_lossy(&after.stdout)
            .replace("\r\n", "\n")
            .trim(),
        "apm--cron"
    );
}
