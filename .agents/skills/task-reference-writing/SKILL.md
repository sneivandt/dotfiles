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

## Establish the task's contract

Read the owning task, its resource or operation, configuration decoder, and
focused tests before changing behavioral claims. Then inspect
[app/catalog.rs](../../../cli/src/app/catalog.rs) for app-owned dependencies and
update/remove membership. Check-command tasks instead come from
[commands/check.rs](../../../cli/src/app/commands/check.rs) and
[app/validation](../../../cli/src/app/validation/).
Do not treat an existing page or upstream manual as proof of dotfiles behavior.

Distinguish these before writing:

- Platform/configuration applicability versus runtime tool and privilege needs.
- Failure-blocking dependencies versus ordering-only and conditional waits.
- Declared edges versus selecting predecessors with `--with-deps`.
- Update behavior versus removal's separate graph and ownership rules.
- Configuration loading failures versus diagnostics emitted after loading.

Use [docs-writing](../docs-writing/SKILL.md) for prose choices and
[docs-publishing](../docs-publishing/SKILL.md) when adding routes or navigation.
These are authoring procedures, not permission to run update, remove, plugin
deployment, package preparation, or an overlay script against the host.

## Write the page for a reader new to the domain

The human-facing convention lives in
[Writing documentation](../../../docs/content/writing-documentation.md#task-reference-pages);
do not make published guides send readers to this skill.
Keep the title, frontmatter, filename, and navigation label aligned. Use
[Home symlinks](../../../docs/content/home-symlinks.md) for a declaration-heavy
example and [ShellCheck](../../../docs/content/shellcheck.md) for a check task.

| Page part | Required content |
|---|---|
| Opening | Define the tool or concept, the task's purpose, and its boundary in a short paragraph. Expand unfamiliar acronyms. |
| Summary table | Selector, command membership, availability, success requirements, ordering-only waits, and implementation source. Check tasks use scope/tools and task dependencies instead. |
| What it does / What it checks | Observable effects, ownership, prerequisites, meaningful failure cases, and important non-goals. Do not narrate every helper. |
| Configuration | Owning declaration/source, one verified example, field meanings, defaults, category/overlay exceptions, and important conflicts. State explicitly when no setting exists. |
| Preview and apply / Run the check | Short command using the canonical selector, setup assumptions, what to inspect, and the next action on failure. Keep side effects beside the command. |
| Removal | Only for state-changing tasks: whether removal is supported, ownership checks, preserved state, and any sequence needed before deleting declarations. |

Use these sections where they fit the task, not as empty boilerplate. Overlay
scripts need their protocol and preview/remove explanation. Check tasks do not
need fictional configuration or removal actions.

Keep the summary ahead of detailed internals. Explain consequential domain
distinctions locally: symlink versus copy, enabling versus starting a unit,
package presence versus upgrades, recipe preparation versus installation, or
user settings versus machine policy. Link to the owning guide for the full
workflow rather than duplicating it.

## Use examples and external references deliberately

- Verify examples against tracked declarations or decoder tests. Label
  synthetic examples and excerpts. Keep exact keys and value types accurate.
- Teach formats with common packages or explicitly illustrative resources,
  not unexplained custom recipes or workstation-specific services. State
  required source files and separate real repository inventories from examples.
- Link the authoritative upstream manual at the first useful concept or in
  nearby explanatory prose. Say what it helps with. Prefer one or two relevant
  references over a generic resource directory; built-in dotfiles concepts may
  need only internal references.
- Check external destinations when adding them. Upstream capabilities can be
  broader than this implementation; state the supported subset rather than
  implying arbitrary upstream options are accepted.
- Explain generated versus editable input. Do not direct readers to edit a
  snapshot, installed copy, or merged manifest as the durable source.
- State whether a task preview queries the network, needs installed tools, or
  executes code when consequential. Link the shared preview scope too; do not
  imply a task filter disables binary self-update or all startup effects.
- Give a useful way to recognize the result or diagnose a common unmet state.
  Inspection commands must use a real declared item or a labeled placeholder.
  Explain expected nonzero statuses where absence is ordinary.
- Link page names directly. Do not tell readers to find a page "under
  Development" or in a sidebar group.

## Maintain the overview and verify

[Task reference](../../../docs/content/task-reference.md) owns the selector
index, shared scheduling terms, check/update distinction, and overall removal
limits. Keep domain explanations on the individual pages. Register children
under its single linked navigation parent without inventing duplicate routes.

Review a page as a direct shared link: can a reader understand the domain,
choose the right declaration, anticipate side effects, and recognize what
success does not prove? Check incoming links after heading changes and select
the native documentation checks from [docs/README.md](../../../docs/README.md#checks).
Browser coverage should preserve task-child navigation, page outlines, search,
and readable tables on narrow screens. Metadata and browser checks do not prove
the behavioral claims or the effectiveness of these instructions.
