# dotfiles APM autopilot helper.
#
# Flips the dotfiles-managed Copilot App workflows to autopilot.
#
# Invoked as: python -c <script> <db_path> <id>...
# The trailing arguments are the dotfiles-managed workflow ids. It first prints
# two space-separated integers -- the number of those rows present and the
# number it actually updated -- then, one per line, the id of every such row now
# in the desired state. parse_autopilot_result reads both parts back. The ids
# are bound as query parameters in an IN (...) clause so the change is scoped to
# exactly the workflows this install deployed, and the IS NOT comparisons are
# NULL-safe.
#
# It also arms the scheduler for each managed row by setting `next_run_at` when it
# is unset or overdue, so the Copilot App actually fires them on schedule.
#
# Schema contract (version 2): the Copilot App sqlite `workflows` table must
# expose `id` (TEXT), `mode` (TEXT) and `enabled` (INTEGER), plus the scheduling
# columns `interval` (TEXT), `schedule_hour`/`schedule_minute`/`schedule_day`
# (INTEGER) and `next_run_at` (TEXT, ISO-8601 UTC). If that contract changes,
# bump this version and update the Rust callers in autopilot.rs.
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

con = sqlite3.connect(sys.argv[1], timeout=5)
con.execute("PRAGMA busy_timeout=5000")
ids = sys.argv[2:]
ph = ",".join("?" for _ in ids)
matched = con.execute("SELECT COUNT(*) FROM workflows WHERE id IN (" + ph + ")", ids).fetchone()[0]
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
