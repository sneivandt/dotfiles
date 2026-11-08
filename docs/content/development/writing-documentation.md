---
title: Writing documentation
description: Choose a guide's scope, verify procedures and examples, write searchable page metadata, and maintain task references for human readers.
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
| How do I install this checkout? | [Getting started](../guides/getting-started.md) |
| What should I run day to day? | [Commands and usage](../guides/commands-and-usage.md) |
| What is the exact flag or output format? | [CLI reference](../reference/cli-reference.md) |
| How do categories, paths, and overlays combine? | [Configuration basics](../guides/configuration-basics.md) |
| What does one task manage, and how is it declared? | Its [task reference page](../reference/task-reference.md) |
| Which checks should I run for a patch? | [Testing](testing.md) |
| How do I construct a regression? | [Writing tests](writing-tests.md) |
| How do I run focused mutations safely? | [Mutation testing](mutation-testing.md) |
| How are custom Arch recipes discovered and prepared? | [Custom package versions](../reference/custom-package-versions.md) and [Custom package repository](../reference/custom-package-repository.md) |

Keep application, package, and agent workflows in their own guides. Separate
contributor implementation contracts from instructions for using the feature. Link to
their full explanation instead of copying it into every reference page.
Brief definitions and safety warnings may repeat where the reader acts.

This guide owns reader-facing content conventions. The site README owns
site implementation and checks. Agent skills route a change to those sources
and add agent-specific safeguards; they should not carry a second copy of
these procedures.

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

## Review the starting state and transition

A valid command is not a complete procedure. Identify whether the reader starts
with a fresh machine, an installed setup, or a partially failed run. For a
migration, describe the state before and after each consequential step.

Check what happens to existing files, link targets, ownership records, saved
selections, and prerequisites. A configuration override does not necessarily
replace installed state. A copied file can block a later link installation.
Changing selection does not undo an earlier update.

Walk the common path and one relevant recovery path against the implementation
or an isolated fixture. State what the reader should observe at each decision,
what must remain available, and when it is safe to retire the old source or
backup. Do not use a real home, deployment, package manager, or desktop restart
as a substitute for a fixture.

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

For an example covered by a regression, keep its language and source filename
in the code fence accurate. The
[documentation contracts](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/docs_contracts.rs)
extract selected TOML examples and pass them through the real loader with
synthetic sources and explicit profiles/platforms. APM's fragment tests check
the YAML examples with its real merger. Add a case when introducing a
consequential declaration or transition; do not copy the snippet into a test
and let the two versions drift. These fixtures prove only the assertions they
make, not a full installation or an upstream application's configuration.

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

Use [Home symlinks](../reference/home-symlinks.md) for a declaration-heavy page and
[ShellCheck](../reference/shellcheck.md) for a check task. The [Task reference](../reference/task-reference.md)
overview owns the selector index and shared scheduling terms; individual pages
own domain details. Link prerequisites by name and distinguish success
requirements from ordering-only waits.

## Help readers find the topic

Treat a page's title, description, and headings as reader-facing metadata,
not administrative boilerplate. The title names the topic in navigation,
browser tabs, and search results. Keep it specific and aligned with the H1,
filename, and route rather than adding a different SEO title.

Write a unique, plain-language description that identifies the reader's task,
the relevant tool or platform, and the important boundary. Include a useful
expanded term alongside an acronym where natural, such as Arch User Repository
and AUR, or HKEY_CURRENT_USER and registry preferences. Avoid descriptions
that merely repeat the title, unsupported promises, lists of unrelated
keywords, or a fixed character count that forces awkward prose.

The description becomes the page's HTML and social-preview summary. Local
search also indexes it once with the page introduction. Section headings and
body text remain searchable and supply direct section results. Use headings
that match likely questions or symptoms; explain unfamiliar terms in the prose
instead of hiding aliases in metadata. There is no separate tags or keywords
catalog, and a `keywords` meta tag does not tune the site's local search.

After changing metadata or topic organization, build the site and search using
reader language as well as the exact title. Check that the intended page is a
prominent result and opens the relevant explanation. Keep representative
queries in the browser regressions without asserting a frozen full ranking.
Canonical URLs and the sitemap identify public routes; the 404 page stays
excluded with `search: false` and `noindex`. Do not exclude an ordinary guide
merely to hide a weak or duplicate explanation.

## Write and review the page

Give each page frontmatter with a nonempty single-line title and description, one matching H1,
and a lowercase hyphenated filename matching the title. Use relative `.md`
links between guides and full GitHub URLs for tracked sources outside them.
Refer to pages by name, not by their position in navigation.

Lead with the common workflow. Prefer direct sentences and headings that name
a task or decision. Reserve inline code for exact syntax and bold for a
consequential distinction. Avoid decorative punctuation and competing emphasis.
Use warning callouts for safety information and explain any highlighted code.

Use readable link labels, including filenames when useful; do not put inline
code inside a link label. Refer to tools and concepts in plain prose, reserving
inline code for exact commands, keys, values, output, and paths.
Use direct sentences and familiar verbs, without canned openings or claims
such as "seamlessly". Avoid em dashes, decorative sentence breaks, heading
ellipses, long semicolon chains, and stacked emphasis. Preserve literal syntax,
quoted output, and meaningful symbols. Vary sentence length naturally instead
of giving every section the same opening and closing formula.

When adding or moving a page, update navigation and incoming links.
Do not add old-name redirects or heading aliases. Register the page in
[site configuration](https://github.com/sneivandt/dotfiles/blob/main/docs/.vitepress/config.mts)
and review its generated outline, previous/next links, and search result.

Choose [documentation checks](testing.md#documentation-site) for the changed
content. Ordinary wording can be reviewed in source with affected link and
metadata checks. Inspect rendered output when syntax, layout, navigation, or
interaction can change, using desktop and narrow widths where relevant.
Follow links as a new reader: they should supply depth, not a missing essential
step. Automated checks cannot establish factual accuracy or whether an example
teaches the right thing.

Keep important lookup headings within the site's outline depth, levels two
and three. Put exceptional recovery steps after the normal procedure, and
make sure instructions after an exception have their own heading if they
apply to everyone. Use consistent task-page sections without copying generic
closing paragraphs from one page to the next.

## Acceptance scenarios

When changing the authoring guide or its agent skills, walk these cases through
the revised guidance. Keep the exercise read-only or use isolated fixtures:

| Reader task | Essential facts the resulting explanation must preserve |
|---|---|
| Switch an installed Git link to an overlay source | Exact-target declaration replacement differs from installed-link replacement; inspect and preserve the old destination, move only the intended link aside, repeat preview, and verify the new origin |
| Retire desktop links and return to base | Removal uses the original selection and copies current sources; base copies block link installation; preserve edits before clearing only the intended conflicts |
| Retry a partial service update | Select the failed run by stable ID, inspect predecessor failures, review dependency expansion, and explain that earlier changes remain |
| Document a task with no removal action | State what stops being managed and what persists; give no fictional remove command or rollback promise |
| Make a reference page discoverable without knowing its selector | Use a unique task-focused description and recognizable tool/platform terms; verify a natural-language query reaches the intended page and section |

Judge whether a reader can complete the task and recognize its outcome, not
whether the explanation matches a fixed sentence or section count.
These cases supplement skill-routing review; neither is a benchmark of an
agent model's effectiveness.
