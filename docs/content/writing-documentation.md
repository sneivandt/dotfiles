---
title: Writing documentation
description: Choose a guide's scope, write reusable examples, and maintain task references for human readers.
---

# Writing documentation

Write for the person trying to complete a task. They should be able to open
a shared page, understand its assumptions, make the next decision, and
recognize the result without reading the whole site first.

This guide covers content and task reference conventions. The
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md)
owns preview commands, navigation implementation, site checks, and publication.

## Choose the owning page

Start with the question the reader has, not the directory the code lives in.
Update an existing explanation when the reader's task has not changed.
Create a separate page for a distinct workflow or audience, not simply because
the existing page is long.

| Reader's question | Owning page |
|---|---|
| How do I install this checkout? | [Getting started](getting-started.md) |
| What should I run day to day? | [Commands and usage](commands-and-usage.md) |
| What is the exact flag or output format? | [CLI reference](cli-reference.md) |
| How do categories, paths, and overlays combine? | [Configuration basics](configuration-basics.md) |
| What does one task manage, and how is it declared? | Its [task reference page](task-reference.md) |
| Which checks should I run for a patch? | [Testing](testing.md) |
| How do I construct a regression? | [Writing tests](writing-tests.md) |

Keep application, package, and agent workflows in their own guides. Link to
their full explanation instead of copying it into every reference page.
Brief definitions and safety warnings may repeat where the reader acts.

## Verify the behavior before explaining it

Read the owning command or task, its resource, the decoder, and relevant
tests. For task availability and dependencies, also inspect
[app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs)
or the check command's separate task list. Existing prose and an upstream
manual are not proof that dotfiles supports the same behavior.

Separate what is declared, what is inspected, and what is changed. Distinguish
an unavailable task from an unmet prerequisite, a failed query from missing
state, and a preview from a guarantee of success. Keep startup and bookkeeping
effects visible where calling a command matters.

Link the relevant upstream manual to explain a tool or format, not to imply
support for every upstream option. Use source links where contributors need
to inspect an implementation, with readable labels rather than raw URLs.
Do not send readers to agent skills as substitutes for human documentation.
Skills are instructions to agents; if a procedure matters to a person,
explain it here or link its owning guide.

## Make examples reusable

Use a small example that teaches one decision. Common package names, a simple
Git preference, or an explicitly fictional application are usually clearer
than a custom recipe or workstation-specific service.

Label the kind of example accurately:

| Kind | What to state |
|---|---|
| Excerpt from this checkout | Link the source and identify omitted context |
| Illustrative configuration | Say it is an example, explain needed files/tools, and verify keys and types against the decoder |
| Placeholder command | Say which values to replace before running |
| Abbreviated code sketch | Identify setup and imports that are intentionally omitted |

For configuration, explain where the snippet belongs and whether to extend an
existing array. Do not invite a second TOML table with the same name or imply
that a few lines replace a complete required file. Include any source file
the declaration assumes, or make that prerequisite explicit.

Keep exact identifiers where the mechanism requires them. A registry value,
winget ID, systemd suffix, or configuration key cannot be made generic by
changing it to an unsupported spelling. Repository inventories and
platform-specific setup should still describe the actual supported content,
clearly separated from examples of the declaration format.

Use short commands with real defaults. State the working directory and shell
when they matter; keep only flags needed for the purpose. Put consequences
such as package upgrades, replacement, elevation, or code execution beside the
command. Never include private overlay content, credentials, or raw machine logs.

## Task reference pages

Open by defining the domain and the task's purpose. Explain important
distinctions locally, such as a symlink versus a copy, enabling versus starting
a service, or installing a missing package versus upgrading existing software.
Do not begin with Rust helper names.

Keep the summary table ahead of detailed behavior. State-changing task tables
identify the selector, command membership, availability, success dependencies,
ordering waits, and implementation source. Check-task tables instead identify
their scope, tool requirements, and dependencies.

| Section | What the reader needs |
|---|---|
| What it does / What it checks | Effects, prerequisites, ownership, failure cases, and important non-goals |
| Configuration | Owning input, a valid example where useful, required fields, defaults, and conflicts |
| Preview and apply / Run the check | A scoped command, setup assumptions, what to inspect, and consequences |
| Removal | Whether it is supported, what ownership is checked, and what remains |

Adapt this structure to the task. Do not invent configuration for a fixed
integration or a removal action for a check. Overlay scripts need their
protocol and code-execution behavior explained.

Use [Home symlinks](home-symlinks.md) for a declaration-heavy page and
[ShellCheck](shellcheck.md) for a check task. The [Task reference](task-reference.md)
overview owns the selector index and shared scheduling terms; individual pages
own domain details. Link prerequisites by name and distinguish success
requirements from ordering-only waits.

## Write and review the page

Give each page frontmatter with a title and description, one matching H1,
and a lowercase hyphenated filename matching the title. Use relative `.md`
links between guides and full GitHub URLs for tracked sources outside them.
Refer to pages by name, not by their position in navigation.

Lead with the common workflow. Prefer direct sentences and headings that name
a task or decision. Reserve inline code for exact syntax and bold for a
consequential distinction. Avoid decorative punctuation and competing emphasis.
Use warning callouts for safety information and explain any highlighted code.

When adding or moving a page, update navigation and incoming links.
Do not add old-name redirects or heading aliases. Register the page in
[site configuration](https://github.com/sneivandt/dotfiles/blob/main/docs/.vitepress/config.mts)
and review its generated outline, previous/next links, and search result.

Run the [documentation checks](testing.md#documentation-site), then read the
rendered page on desktop and a narrow screen. Follow links as a new reader:
they should supply depth, not a missing essential step. Automated checks
cannot establish factual accuracy or whether an example teaches the right thing.
