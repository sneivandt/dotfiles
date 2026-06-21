---
name: project-hygiene
description: Use after code, configuration, workflow, or tooling changes to check whether related docs, tests, skills, prompts, config, schemas, CI, or validation commands need updates.
---

# Project Hygiene

Use this skill after changing code, configuration, workflows, or tooling.

## Follow-Up Checks

- Check whether directly related docs, examples, tests, config, schemas, skills, prompts, CI, release notes, or validation commands now need updates.
- Update related artifacts only when the code or config change makes them outdated, incomplete, misleading, or inconsistent.
- Prefer small, targeted follow-up edits over broad rewrites.
- Add or adjust tests when behavior changes and existing test patterns support it.
- Use existing validation commands; do not introduce new tooling unless necessary.
- Preserve unrelated files and avoid cleanup that is not tied to the change.
- If no follow-up update is needed, say that plainly and briefly.
