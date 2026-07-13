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
#
# Schema contract (version 3): the Copilot App sqlite `workflows` table must
# expose `id`, `name`, `prompt`, `mode`, and `enabled`, plus the scheduling
# columns `interval`, `schedule_hour`/`schedule_minute`/`schedule_day` and
# `next_run_at` (TEXT, ISO-8601 UTC). If that contract changes, bump this
# version and update the Rust callers in autopilot.rs.
import sqlite3, sys
from datetime import datetime, timedelta, timezone


def compute_next_run(interval, hour, minute, day, now_local):
    """Next scheduled fire time as an ISO-8601 UTC string, or None for manual.

    schedule_hour/minute/day are interpreted in machine-local time (matching the
    Copilot App) and converted to UTC. schedule_day is 0=Sunday..6=Saturday.
    """
    if interval == "hourly":
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
# is left untouched to avoid rescheduling on every install; manual rows are skipped.
now_local = datetime.now().astimezone()
now_utc = datetime.now(timezone.utc)
sched = con.execute("SELECT id, interval, schedule_hour, schedule_minute, schedule_day, next_run_at FROM workflows WHERE id IN (" + ph + ")", ids).fetchall()
for wid, interval, hour, minute, day, nra in sched:
    target = compute_next_run(interval, 9 if hour is None else hour, 0 if minute is None else minute, 1 if day is None else day, now_local)
    if target is None:
        continue
    existing = parse_utc(nra)
    if existing is None or existing <= now_utc:
        con.execute("UPDATE workflows SET next_run_at=? WHERE id=?", (target, wid))
con.commit()
print(matched, cur.rowcount)
for row in con.execute("SELECT id FROM workflows WHERE id IN (" + ph + ") AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id", ids):
    print(row[0])
