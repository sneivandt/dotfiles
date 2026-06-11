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
# Schema contract (version 1): the Copilot App sqlite `workflows` table must
# expose `id` (TEXT), `mode` (TEXT) and `enabled` (INTEGER) columns. If that
# contract changes, bump this version and update the Rust callers in autopilot.rs.
import sqlite3, sys

con = sqlite3.connect(sys.argv[1], timeout=5)
con.execute("PRAGMA busy_timeout=5000")
ids = sys.argv[2:]
ph = ",".join("?" for _ in ids)
matched = con.execute("SELECT COUNT(*) FROM workflows WHERE id IN (" + ph + ")", ids).fetchone()[0]
cur = con.execute("UPDATE workflows SET mode='autopilot', enabled=1 WHERE id IN (" + ph + ") AND (mode IS NOT 'autopilot' OR enabled IS NOT 1)", ids)
con.commit()
print(matched, cur.rowcount)
for row in con.execute("SELECT id FROM workflows WHERE id IN (" + ph + ") AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id", ids):
    print(row[0])
