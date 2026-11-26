---
name: docs-writing
description: >
  Use when drafting, auditing, or editing human-facing prose in README.md,
  docs/content/, or application READMEs: clarity, factual accuracy, natural
  language, examples, task reference pages, search metadata, and the right level
  of detail. Not for site navigation, rendering, or publishing alone
  (docs-publishing), or internal agent instructions (skill-maintenance).
---

# Documentation Writing

[Writing documentation](../../../docs/content/development/writing-documentation.md)
owns prose, example, findability, and task-page conventions; use
[docs-publishing](../docs-publishing/SKILL.md) for site organization or
publishing.

## Repo-specific rules

- Never link to `.agents/skills/` from human documentation (checked by
  `check-doc-site.py`). Link directly to target pages rather than naming sidebar
  sections, and keep consequential warnings beside the action rather than behind
  a link.
- Keep `README.md` concise and stable—only edit it when a fact, managed-area
  entry, link, or `terminal-screenshot.svg` output has drifted.
- Describe the current setup directly (skip past migration narration and
  exhaustive default third-party GUI shortcut lists).
- Review procedures as state transitions from both fresh and already-managed
  states, including partial-failure recovery and materialized-copy/conflict
  limits.
- For task reference pages ([index](../../../docs/content/reference/task-reference.md)),
  verify membership and edges in [app/catalog.rs](../../../cli/src/app/catalog.rs)
  (and [commands/check.rs](../../../cli/src/app/commands/check.rs) /
  [app/validation](../../../cli/src/app/validation/) for check tasks). Keep
  these distinctions explicit where relevant: applicability vs runtime tools/privileges,
  blocking vs ordering-only/conditional edges, `--with-deps` expansion,
  separate `update` vs `remove` graphs, load errors vs post-load diagnostics,
  and upstream capabilities exceeding dotfiles' supported subset.

## Verification

Reuse or extend [cli/tests/docs_contracts.rs](../../../cli/tests/docs_contracts.rs)
for consequential config examples or state transitions, and select checks from
[docs/README.md](../../../docs/README.md#checks). Apply the guide's
[acceptance scenarios](../../../docs/content/development/writing-documentation.md#acceptance-scenarios)
when changing authoring guidance.
