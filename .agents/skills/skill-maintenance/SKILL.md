---
name: skill-maintenance
description: >
  Use when creating, reviewing, or editing this repository's .agents/skills/
  or AGENTS.md: skill triggers, routing, instruction quality, and guidance drift.
  Not for APM deployment or reusable personal plugin placement.
---

# Skill Maintenance

## Discoverability and frontmatter

- Store each repo skill at `.agents/skills/<name>/SKILL.md` with a unique
  plain lowercase hyphenated `name` matching the directory and a
  two-space-indented YAML block `description` (`>` or `|`). See
  [scripts/README.md](scripts/README.md) for the validator's strict subset.
- Put concrete paths, task types, and boundary exclusions in `description` so
  selection routes cleanly before bodies load. Give each skill one primary
  responsibility.
- Keep repository-local skills independent of personal APM plugins.

## Content bar

- Keep only non-obvious subsystem invariants, footguns, coupled files, and
  focused test pointers. Do not copy APIs, file/enum inventories, `AGENTS.md`
  rules, or general engineering advice a modern agent already follows.
- Keep commands/CI matrix in [Testing](../../../docs/content/development/testing.md),
  site checks in [docs/README.md](../../../docs/README.md#checks), and human
  workflows per [guidance ownership](../../../docs/content/development/contributing.md#documentation).
- Update incoming references whenever merging, renaming, or deleting a skill.

## Verification

Check changed triggers against [routing-cases.md](routing-cases.md) (and
[writing-documentation.md#acceptance-scenarios](../../../docs/content/development/writing-documentation.md#acceptance-scenarios)
for docs guidance). Run `check-skills.py`, `test-check-skills.py`, and
`check-doc-links.py` (see [docs/README.md](../../../docs/README.md#checks)).
