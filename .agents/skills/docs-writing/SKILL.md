---
name: docs-writing
description: >
  Use when drafting, auditing, or editing human-facing prose in README.md,
  docs/content/, or application READMEs: clarity, factual accuracy, natural
  language, examples, task reference pages, search metadata, and the right level
  of detail. Not for site navigation,
  rendering, or publishing alone (docs-publishing), or internal agent
  instructions (skill-maintenance).
---

# Documentation Writing

The human-facing convention lives in
[Writing documentation](../../../docs/content/development/writing-documentation.md).
Read the relevant sections rather than carrying a separate style policy here.
Use [docs-publishing](../docs-publishing/SKILL.md) for organization or site
behavior. For task reference pages, use the supporting
[implementation checklist](task-reference.md).

## Follow the reader's task

1. Identify the reader, their starting state, and the decision or action they
   need next. Update the owning explanation when the task has not changed.
2. Verify changed claims against the owning configuration, implementation,
   and focused tests. Existing prose and upstream capabilities are not proof.
3. Review a procedure as a state transition, especially on an already-installed
   machine or after partial failure. Check preserved content, prerequisites,
   source availability, saved selections, and how the reader recognizes success.
4. Draft using the guide's example, language, punctuation, link, and emphasis
   conventions. Keep consequential warnings beside the action. Do not replace
   a safety explanation with a link or remove it merely to shorten a page.
   Review its [findability guidance](../../../docs/content/development/writing-documentation.md#help-readers-find-the-topic)
   when changing titles, descriptions, headings, or topic terminology.
5. Read the result as a direct shared link. Its links should supply depth, not
   a missing essential step. Prefer the common workflow before internals and
   recovery cases.

## Verify the result safely

- For consequential examples, reuse or extend the isolated
  [documentation contracts](../../../cli/tests/docs_contracts.rs).
  Distinguish decoder, transition, serialization, and rendered-site evidence.
- Select checks from [docs/README.md](../../../docs/README.md#checks).
  Run example contracts when their inputs or claims change.
- Inspect rendered content when layout, Markdown syntax, tables, diagrams, or
  interaction can change. Use desktop and narrow widths where relevant. Plain
  wording can be reviewed in source; structural checks never prove accuracy.
- Apply the [documentation acceptance scenarios](../../../docs/content/development/writing-documentation.md#acceptance-scenarios)
  when changing authoring guidance. They are manual acceptance cases, not a
  model-effectiveness benchmark.
