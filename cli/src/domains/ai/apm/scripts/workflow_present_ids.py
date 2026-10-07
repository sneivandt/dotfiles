# Read-only scope probe: stale APM lock entries can outlive their workflow rows.
import sqlite3, sys
from pathlib import Path

con = sqlite3.connect(Path(sys.argv[1]).resolve().as_uri() + "?mode=ro", uri=True, timeout=5)
con.execute("PRAGMA busy_timeout=5000")
ids = sys.argv[2:]
ph = ",".join("?" for _ in ids)
for row in con.execute(
    "SELECT DISTINCT id FROM workflows WHERE id IN (" + ph + ") ORDER BY id", ids
):
    print(row[0])
