---
name: ship
description: >
  Use when the user explicitly says "ship" or "ship it" (including "fix this
  and ship it") to carry the active task through delivery and verified checks.
---

# Ship

1. Pick up the active task (or ask if none is clear) and carry out the
   remaining authorized steps, fixing in-scope failures and checking current
   state before retrying partial actions.
2. An explicit "ship" command authorizes delivering repository work through the
   user's requested route or the repo's established PR/direct-push workflow
   (`AGENTS.md`/contributing guide). Verify branch and destination before
   pushing; if the delivery route is unclear, finish local work and ask.
3. Monitor pending checks or external runs against the exact revision/artifact
   until their outcome is verified. Close with what was achieved and how it was
   verified, or the exact blocker if progress stops.
