---
name: ship
description: >
  Use when the user says "ship" or "ship it" as a command,
  including requests such as "fix this and ship it", or asks to see a task
  or run through verified completion.
---

# Ship

1. Pick up the active task when the user gives the command by itself. Use the
   latest instructions and existing authorization to identify the remaining
   work and the observable result that would count as done. If there is no
   active or named task, ask what to take through completion.
2. Carry out the remaining authorized steps. Make reasonable, reversible
   decisions. If a step fails, inspect the failure, fix what is within scope,
   and retry when there is a reason to expect a different result. Check the
   current state before retrying an action that may have partially succeeded.
3. Follow pending checks or external jobs in the current task. Confirm the
   result against the exact revision, run, or artifact involved. Verify the
   outcome through the path the user cares about when available. Share updates
   when the state changes meaningfully.
4. An explicit command to ship repository work authorizes delivery through
   the user's requested route or the repository's established PR or direct-push
   workflow. Check the applicable `AGENTS.md` or contributing guide, verify the
   current branch and destination before any push, and follow the checks through
   the requested endpoint. A request to monitor a run ends when its outcome is
   verified. If the delivery route is unclear, finish independent work and ask
   for the missing decision.
5. Close with what was achieved and how it was verified. If a blocker prevents
   further progress, report what is complete, the exact blocked step, and what
   would let work resume.
