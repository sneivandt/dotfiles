# Task reference implementation checklist

[Writing documentation](../../../docs/content/development/writing-documentation.md#task-reference-pages)
owns the reader-facing page structure, examples, and language.
[Task reference](../../../docs/content/reference/task-reference.md) owns the selector
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

## Explain the domain contract

Use the human guide's task-page sections where they fit, without empty
boilerplate. Explain the consequential domain distinction locally, such as
enablement versus starting, package presence versus upgrades, or links versus
copies. Upstream support can exceed dotfiles' supported subset.

Review the procedure from both the relevant fresh and already-managed states.
For replacement, removal, and partial-failure recovery, trace the state left
after each step and what can block the next one. Show how to recognize the
result, including ordinary nonzero inspection statuses.

Extend the isolated [documentation contracts](../../../cli/tests/docs_contracts.rs)
for consequential examples and transitions. Decoder and browser checks do not
prove an upstream integration or a full machine installation.
