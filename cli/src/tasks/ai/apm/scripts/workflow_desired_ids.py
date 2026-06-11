# dotfiles APM autopilot helper (read-only).
#
# Lists which of the dotfiles-managed Copilot App workflows are already in the
# desired state (mode='autopilot', enabled=1).
#
# Invoked as: python -c <script> <db_path> <id>...
# The trailing arguments are the dotfiles-managed workflow ids; they are bound
# as query parameters in an IN (...) clause and matches are printed one id per
# line in id order, which parse_desired_ids reads back.
#
# Schema contract (version 1): the Copilot App sqlite `workflows` table must
# expose `id` (TEXT), `mode` (TEXT) and `enabled` (INTEGER) columns. If that
# contract changes, bump this version and update the Rust callers in autopilot.rs.
import sqlite3, sys

con = sqlite3.connect(sys.argv[1], timeout=5)
con.execute("PRAGMA busy_timeout=5000")
ids = sys.argv[2:]
ph = ",".join("?" for _ in ids)
q = "SELECT id FROM workflows WHERE id IN (" + ph + ") AND mode IS 'autopilot' AND enabled IS 1 ORDER BY id"
for row in con.execute(q, ids):
    print(row[0])
