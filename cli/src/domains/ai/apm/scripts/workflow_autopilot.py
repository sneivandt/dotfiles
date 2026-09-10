# dotfiles APM autopilot helper.
#
# Flips the dotfiles-managed Copilot App workflows to autopilot.
#
# Invoked as: python -c <script> <db_path> <id>...
# The trailing arguments are the dotfiles-managed workflow ids. It first removes
# duplicate rows for those managed workflow definitions, then prints two
# space-separated integers -- the number of those rows present and the number it
# actually updated -- then, one per line, the id of every such row now in the
# desired state. parse_autopilot_result reads both parts back. The ids are bound
# as query parameters in an IN (...) clause so the change is scoped to exactly the
# workflows this install deployed, and the IS NOT comparisons are NULL-safe.
#
# It also removes duplicate rows for each managed workflow id before arming the
# scheduler by setting `next_run_at` when it is unset or overdue, so the Copilot
# App shows one automation card per APM workflow and actually fires it on schedule.
# Custom cron schedules are stored as interval='manual' plus cron_expression and
# must be handled before the ordinary interval schedule.
#
# Schema contract (version 4): the Copilot App sqlite `workflows` table must
# expose `id`, `name`, `prompt`, `mode`, and `enabled`, plus the scheduling
# columns `interval`, `schedule_hour`/`schedule_minute`/`schedule_day`, and
# `next_run_at` (TEXT, ISO-8601 UTC). `cron_expression` is additive and detected
# at runtime so older App schemas retain interval-only repair. If the required
# contract changes, bump this version and update the Rust callers in autopilot.rs.
import sqlite3, sys
from datetime import datetime, timedelta, timezone


def parse_cron_field(field, minimum, maximum, *, sunday_alias=False):
    """Expand one numeric cron field supporting lists, ranges, and steps."""
    values = set()
    for item in field.split(","):
        base, separator, step_text = item.partition("/")
        step = int(step_text) if separator else 1
        if step <= 0:
            raise ValueError(f"invalid cron step: {item!r}")
        if base == "*":
            start, end = minimum, maximum
        elif "-" in base:
            start_text, end_text = base.split("-", 1)
            start, end = int(start_text), int(end_text)
        else:
            start = int(base)
            end = maximum if separator else start
        if start < minimum or end > maximum or start > end:
            raise ValueError(f"invalid cron range: {item!r}")
        values.update(range(start, end + 1, step))
    if sunday_alias and 7 in values:
        values.remove(7)
        values.add(0)
    return values


def compute_next_cron(expression, now_local):
    """Compute the next local occurrence of a standard five-field cron."""
    fields = expression.split()
    if len(fields) != 5:
        raise ValueError(f"expected five cron fields, got {len(fields)}")
    minute_text, hour_text, month_day_text, month_text, week_day_text = fields
    minutes = parse_cron_field(minute_text, 0, 59)
    hours = parse_cron_field(hour_text, 0, 23)
    month_days = parse_cron_field(month_day_text, 1, 31)
    months = parse_cron_field(month_text, 1, 12)
    week_days = parse_cron_field(week_day_text, 0, 7, sunday_alias=True)
    month_day_any = month_day_text == "*"
    week_day_any = week_day_text == "*"

    candidate = now_local.replace(second=0, microsecond=0) + timedelta(minutes=1)
    for _ in range(8 * 366 * 24 * 60):
        if candidate.month not in months:
            candidate += timedelta(minutes=1)
            continue
        month_day_matches = candidate.day in month_days
        week_day_matches = ((candidate.weekday() + 1) % 7) in week_days
        if month_day_any:
            day_matches = week_day_matches
        elif week_day_any:
            day_matches = month_day_matches
        else:
            day_matches = month_day_matches or week_day_matches
        if (
            day_matches
            and candidate.hour in hours
            and candidate.minute in minutes
        ):
            return candidate
        candidate += timedelta(minutes=1)
    raise ValueError(f"cron has no occurrence in the next eight years: {expression!r}")


def compute_next_run(interval, cron_expression, hour, minute, day, now_local):
    """Next scheduled fire time as an ISO-8601 UTC string, or None for manual.

    schedule_hour/minute/day are interpreted in machine-local time (matching the
    Copilot App) and converted to UTC. schedule_day is 0=Sunday..6=Saturday.
    """
    if cron_expression and cron_expression.strip():
        nxt = compute_next_cron(cron_expression.strip(), now_local)
    elif interval == "hourly":
        nxt = now_local.replace(minute=minute, second=0, microsecond=0)
        if nxt <= now_local:
            nxt += timedelta(hours=1)
    elif interval == "daily":
        nxt = now_local.replace(hour=hour, minute=minute, second=0, microsecond=0)
        if nxt <= now_local:
            nxt += timedelta(days=1)
    elif interval == "weekly":
        nxt = now_local.replace(hour=hour, minute=minute, second=0, microsecond=0)
        cur_dow = (nxt.weekday() + 1) % 7  # Python Mon=0..Sun=6 -> app Sun=0..Sat=6
        nxt += timedelta(days=(day - cur_dow) % 7)
        if nxt <= now_local:
            nxt += timedelta(days=7)
    else:
        return None
    return nxt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


def parse_utc(value):
    """Parse a stored next_run_at into an aware UTC datetime, or None."""
    if not value:
        return None
    text = value.strip()
    if text.endswith("Z"):
        text = text[:-1]
    if "." in text:
        text = text.split(".", 1)[0]
    try:
        return datetime.strptime(text, "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
    except ValueError:
        return None


def table_has_rowid(connection):
    """Whether the workflows table exposes SQLite's implicit rowid."""
    try:
        connection.execute("SELECT rowid FROM workflows LIMIT 0")
    except sqlite3.OperationalError as exc:
        if "no such column: rowid" in str(exc):
            return False
        raise
    return True


def dedupe_managed_workflows(connection, workflow_ids, placeholders):
    """Remove duplicate rows for managed definitions, keeping current APM ids."""
    if not table_has_rowid(connection):
        return

    # First collapse exact id duplicates from repeated APM installs.
    connection.execute(
        "DELETE FROM workflows "
        "WHERE id IN (" + placeholders + ") "
        "AND rowid NOT IN ("
        "SELECT MAX(rowid) FROM workflows WHERE id IN (" + placeholders + ") GROUP BY id"
        ")",
        workflow_ids + workflow_ids,
    )

    # Then collapse cross-id duplicates for the same visible automation
    # definition. This handles older APM rows such as apm--unknown--... that
    # predate the current _local id but render as the same card in the app.
    managed_defs = connection.execute(
        "SELECT name, prompt, interval, schedule_hour, schedule_minute, schedule_day "
        "FROM workflows WHERE id IN (" + placeholders + ")",
        workflow_ids,
    ).fetchall()
    for definition in managed_defs:
        rows = connection.execute(
            "SELECT rowid, id FROM workflows "
            "WHERE name IS ? AND prompt IS ? AND interval IS ? AND schedule_hour IS ? "
            "AND schedule_minute IS ? AND schedule_day IS ?",
            definition,
        ).fetchall()
        if len(rows) <= 1:
            continue
        managed_rows = [row for row in rows if row[1] in workflow_ids]
        keep = max(managed_rows or rows, key=lambda row: row[0])
        for rowid, _workflow_id in rows:
            if rowid != keep[0]:
                connection.execute("DELETE FROM workflows WHERE rowid=?", (rowid,))

con = sqlite3.connect(sys.argv[1], timeout=5)
con.execute("PRAGMA busy_timeout=5000")
ids = sys.argv[2:]
ph = ",".join("?" for _ in ids)
matched = con.execute("SELECT COUNT(*) FROM workflows WHERE id IN (" + ph + ")", ids).fetchone()[0]
dedupe_managed_workflows(con, ids, ph)
cur = con.execute("UPDATE workflows SET mode='autopilot', enabled=1 WHERE id IN (" + ph + ") AND (mode IS NOT 'autopilot' OR enabled IS NOT 1)", ids)
# Arm the scheduler: set next_run_at on managed rows that are unarmed (NULL) or
# overdue (<= now), so the app fires them on schedule. A valid future next_run_at
# is left untouched to avoid rescheduling on every install; manual rows without a
# custom cron expression are skipped.
now_local = datetime.now().astimezone()
now_utc = datetime.now(timezone.utc)
columns = {row[1] for row in con.execute("PRAGMA table_info(workflows)")}
cron_column = "cron_expression" if "cron_expression" in columns else "NULL"
sched = con.execute(
    "SELECT id, interval, " + cron_column + ", schedule_hour, schedule_minute, "
    "schedule_day, next_run_at FROM workflows WHERE id IN (" + ph + ")",
    ids,
).fetchall()
for wid, interval, cron_expression, hour, minute, day, nra in sched:
    target = compute_next_run(
        interval,
        cron_expression,
        9 if hour is None else hour,
        0 if minute is None else minute,
        1 if day is None else day,
        now_local,
    )
    if target is None:
        continue
    existing = parse_utc(nra)
    if existing is None or existing <= now_utc:
        con.execute("UPDATE workflows SET next_run_at=? WHERE id=?", (target, wid))
con.commit()
print(matched, cur.rowcount)
cron_empty = (
    "(cron_expression IS NULL OR trim(cron_expression) = '')"
    if "cron_expression" in columns
    else "1"
)
desired = (
    "SELECT id FROM workflows WHERE id IN (" + ph + ") "
    "AND mode IS 'autopilot' AND enabled IS 1 "
    "AND ((interval IS 'manual' AND " + cron_empty + ") "
    "OR next_run_at > strftime('%Y-%m-%dT%H:%M:%fZ','now')) ORDER BY id"
)
for row in con.execute(desired, ids):
    print(row[0])
