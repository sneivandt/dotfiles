# dotfiles APM autopilot helper (read-only).
#
# Lists which of the dotfiles-managed Copilot App workflows are already in the
# desired state (mode='autopilot', enabled=1, and an armed schedule when the
# workflow is scheduled).
#
# Invoked as: python -c <script> <db_path> <id>...
# The trailing arguments are the dotfiles-managed workflow ids; they are bound
# as query parameters in an IN (...) clause and matches are printed one id per
# line in id order, which parse_desired_ids reads back.
#
# Schema contract (version 3): the Copilot App sqlite `workflows` table must
# expose `id`, `mode`, `enabled`, `interval`, and `next_run_at`.
# `cron_expression` is additive and detected at runtime so older App schemas
# retain interval-only state checks. If the required contract changes, bump this
# version and update the Rust callers in autopilot.rs.
import sqlite3, sys

con = sqlite3.connect(sys.argv[1], timeout=5)
con.execute("PRAGMA busy_timeout=5000")
ids = sys.argv[2:]
ph = ",".join("?" for _ in ids)
columns = {row[1] for row in con.execute("PRAGMA table_info(workflows)")}
cron_empty = (
    "(cron_expression IS NULL OR trim(cron_expression) = '')"
    if "cron_expression" in columns
    else "1"
)
q = (
    "SELECT id FROM workflows WHERE id IN (" + ph + ") "
    "AND mode IS 'autopilot' AND enabled IS 1 "
    "AND ((interval IS 'manual' AND " + cron_empty + ") "
    "OR next_run_at > strftime('%Y-%m-%dT%H:%M:%fZ','now')) ORDER BY id"
)
for row in con.execute(q, ids):
    print(row[0])
