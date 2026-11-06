---
name: task-reference-writing
description: >
  Use when drafting, expanding, or reviewing docs/content/task-reference.md
  or individual task reference pages: domain context, summary tables,
  configuration examples, dependencies, preview limits, removal, and upstream
  tool links. Not for general guide prose (docs-writing), contributor workflow
  guides, or site navigation/rendering alone (docs-publishing).
---

# Task Reference Writing

[Writing documentation](../../../docs/content/writing-documentation.md#task-reference-pages)
owns the reader-facing page structure, examples, and language.
[Task reference](../../../docs/content/task-reference.md) owns the selector
index and shared selection/scheduling terms. Do not duplicate those procedures
here or send human readers to a skill as required reading.

## Establish the changed contract

Read the owning task, resource or operation, decoder, and focused tests.
Inspect [app/catalog.rs](../../../cli/src/app/catalog.rs) for app-owned
dependencies and update/remove membership. Check tasks instead come from
[commands/check.rs](../../../cli/src/app/commands/check.rs) and
[app/validation](../../../cli/src/app/validation/).

Verify these distinctions before writing:

- Platform/configuration applicability versus runtime tools and privileges.
- Blocking dependencies versus ordering-only and conditional waits.
- Declared graph edges versus selection expansion with `--with-deps`.
- Update versus removal's separate graph and ownership rules.
- Loading errors versus diagnostics emitted after loading.
- Editable declarations and sources versus installed or generated output.
- A task's proposed work versus startup, network, bookkeeping, and script effects.

## Explain and exercise the reader's workflow

Use the human guide's task-page sections where they fit, without empty
boilerplate. Explain the consequential domain distinction locally, such as
enablement versus starting, package presence versus upgrades, or links versus
copies. Upstream support can exceed dotfiles' supported subset.
Write a useful description naming the task, tool/platform, and boundary, using
the human guide's [findability conventions](../../../docs/content/writing-documentation.md#help-readers-find-the-topic).
Verify a natural query reaches the page, not only an exact selector lookup.

Review the procedure from both the relevant fresh and already-managed states.
For replacement, removal, and partial-failure recovery, trace the state left
after each step and what can block the next one. Show how to recognize the
result, including ordinary nonzero inspection statuses.

Use [docs-writing](../docs-writing/SKILL.md) for prose and
[docs-publishing](../docs-publishing/SKILL.md) for route/navigation changes.
Register task pages without duplicating routes or the overview.

Extend the isolated [documentation contracts](../../../cli/tests/docs_contracts.rs)
for consequential examples and transitions. Choose focused and final native
documentation coverage from [docs/README.md](../../../docs/README.md#checks).
Decoder and browser checks do not prove an upstream integration or a full
machine installation. Examples never authorize host updates, removals,
package preparation, overlay execution, or deployment.
