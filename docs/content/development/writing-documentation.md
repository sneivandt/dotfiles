---
title: Writing documentation
description: Choose a guide's scope, verify procedures and examples, write searchable page metadata, and maintain task references for human readers.
---

# Writing documentation

Write for someone trying to get a job done. A reader who lands on any page
should immediately understand its assumptions, know what to do next, and
recognize the expected result without having to read the entire site first.

This guide covers prose, examples, search metadata, and task-reference
conventions. For local site previews, VitePress wiring, checks, and publishing,
see the [site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md).

## Choose the owning page

Start from the reader's question rather than the codebase layout. Update an
existing guide when the workflow is the same, and split out a new page only for
a distinct workflow or audience, not just because a page is getting long.

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

Keep application, package, and agent workflows in their dedicated guides, and
keep contributor implementation details out of user-facing instructions. Link
to the full explanation instead of repeating it on every reference page, while
still keeping short definitions and safety warnings right where the reader
acts.

Human content standards live here, site build mechanics live in the site
README, and agent skills route work to both without carrying a duplicate copy
of these procedures.

## Verify the behavior before explaining it

Before writing, read the owning command or task, its resource, its config
decoder, and its tests. For task eligibility and dependencies, also check
[app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs)
or the `check` command's task list. Never rely solely on older docs or an
upstream tool's manual to assume what dotfiles supports.

Be clear about what is declared, what is inspected, and what is changed.
Distinguish an inapplicable task from an unmet prerequisite, a failed query
from missing state, and a dry-run preview from a guarantee that apply will
succeed. Call out startup and bookkeeping effects (like saving a profile or
overlay path) wherever running a command triggers them.

Link upstream manuals to clarify a tool or format without implying dotfiles
exposes every upstream option. Link repository source files with readable
labels when contributors need to read the implementation. Never send human
readers to agent skills as documentation; if a procedure matters to a person,
write it in the owning guide.

## Review the starting state and transition

A valid command isn't a complete procedure by itself. State whether the reader
starts from a fresh machine, an existing install, or a partially failed run,
and describe the before-and-after state of each consequential step in a
migration.

Trace what happens to existing files, symlink targets, ownership records,
saved selections, and prerequisites. Changing a declaration doesn't
automatically retarget an installed symlink, a copied file can block a later
link, and switching profiles won't undo a previous update.

Walk both the happy path and a realistic recovery path against the code or an
isolated fixture. Tell the reader what to check at each decision point, what
must stay in place, and when it is safe to delete a backup or old source. Never
test procedures against your live home directory, package manager, or desktop
session when an isolated fixture will do.

## Make examples reusable

Choose small examples that illustrate one decision at a time: familiar package
names, a simple Git setting, or a clearly fictional app usually teach faster
than a custom recipe or machine-specific unit.

Label every example clearly:

| Kind | What to state |
|---|---|
| Excerpt from this checkout | Link the source and note omitted context |
| Illustrative configuration | Mark it as an example, note required files or tools, and verify keys and types against the decoder |
| Placeholder command | Name every placeholder the reader must replace before running |
| Abbreviated code sketch | Note setup and imports that are intentionally omitted |

For configuration snippets, state which file they belong in and whether to
extend an existing array rather than duplicating a TOML table header or
replacing a whole file. Mention any source file the declaration expects.

Keep exact identifiers wherever the underlying mechanism requires them (such as
registry value types, WinGet IDs, systemd unit suffixes, or config keys). Keep
real repository inventories clearly separated from generic format examples.

When an example is verified by automated tests, keep its code-fence language
and `title` filename intact. The
[documentation contracts](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/docs_contracts.rs)
parse selected TOML and JSON blocks through the real loaders using synthetic
sources, and APM fragment tests validate the YAML blocks through the real
merger. Add a test case for any new consequential declaration instead of
copying snippets into disconnected fixtures that can drift.

Keep commands short with realistic defaults, noting working directory and shell
when they matter. Place high-consequence warnings (upgrades, replacements,
elevation, or script execution) right next to the command, and never include
private overlay content, credentials, or raw machine logs.

## Task reference pages

Start by explaining the domain and what the task manages in plain terms,
clarifying key distinctions up front (such as symlink vs. copy, enabling vs.
starting a service, or installing missing packages vs. upgrading installed
ones) rather than opening with Rust struct names.

Place the metadata summary table before detailed sections. State-changing task
tables list the selector, command membership, availability, blocking
dependencies, ordering waits, and source link; check-task tables list scope,
tool requirements, and dependencies.

| Section | What the reader needs |
|---|---|
| What it does / What it checks | Effects, prerequisites, ownership, failure modes, and explicit non-goals |
| Configuration | Owning file, a valid example where helpful, required fields, defaults, and conflict rules |
| Preview and apply / Run the check | A scoped command, starting assumptions, what to inspect, and side effects |
| Removal | Whether removal is supported, what ownership is checked, and what remains behind |

Adapt those headings to the task rather than inventing config sections for
fixed integrations or removal sections for checks. Overlay scripts also need
their protocol and code-execution risks explained clearly.

Use [Home symlinks](../reference/home-symlinks.md) as a model for a
declaration-heavy page and [ShellCheck](../reference/shellcheck.md) for a check
task. Keep the selector index and shared scheduling model on
[Task reference](../reference/task-reference.md) and domain details on each
task page, linking prerequisites by name and distinguishing blocking
dependencies from ordering-only waits.

## Help readers find the topic

Treat page titles, descriptions, and headings as reader-facing navigation aids
rather than boilerplate. Keep the `title` specific and aligned with the H1,
filename, and URL instead of inventing a separate SEO title.

Write a unique, plain-language `description` naming the reader's goal, the
tool or platform involved, and any key boundary. Pair acronyms with their
expanded names where natural (such as Arch User Repository and AUR, or
HKEY_CURRENT_USER and registry preferences). Avoid descriptions that merely
echo the title, pad out keywords, or strain prose to hit a character count.

The description feeds HTML metadata, social previews, and local search
alongside headings and body prose. Write headings that mirror real questions
and symptoms, and define unfamiliar terms in the body rather than trying to
stuff aliases into metadata (local search does not use a separate `keywords`
tag).

After changing metadata or navigation, build the site and test natural-language
queries alongside exact titles to confirm the right page surfaces prominently.
Keep representative queries in browser tests without pinning brittle full
rankings. Public routes keep canonical and social metadata, while only the
landing page (`/`) is listed in the sitemap and marked indexable; guide,
reference, and development pages emit `noindex, follow`, and `404` stays
excluded via `search: false` and `noindex, nofollow`.

## Write and review the page

Give every page frontmatter with a nonempty single-line `title` and
`description`, a single matching H1, and a lowercase hyphenated filename. Use
relative `.md` links between docs pages and full GitHub URLs for tracked repo
files. Refer to other pages by name rather than by sidebar position.

Lead with the common path using direct sentences and task-oriented headings.
Reserve inline code for exact syntax and bold for consequential distinctions.
Use `::: warning` callouts for safety notes and explain highlighted code.

Write readable link labels (filenames are fine) without inline code inside the
label. Refer to tools and concepts in plain prose, reserving inline code for
commands, keys, values, output, and paths. Avoid canned filler like
"seamlessly", em dashes, decorative sentence breaks, heading ellipses, long
semicolon chains, and stacked emphasis. Preserve literal syntax and quoted
output, and vary sentence rhythm naturally rather than repeating the same
formula in every section.

When adding or moving a page, update
[site configuration](https://github.com/sneivandt/dotfiles/blob/main/docs/.vitepress/config.mts)
and all incoming links without adding old-name redirects or alias headings,
then review the rendered outline, prev/next links, and search results.

Pick verification steps from
[Documentation site checks](testing.md#documentation-site). Pure prose edits
often only need link and metadata checks, while layout, syntax, navigation, or
search changes benefit from inspecting the built site at desktop and narrow
widths. Read links like a newcomer: links should add depth rather than hiding a
mandatory step, and automated tests never replace verifying factual accuracy.

Keep key lookup headings at levels two and three so they appear in the page
outline. Place rare recovery paths after the main procedure, and start a fresh
heading afterward if later instructions apply to everyone.

## Acceptance scenarios

When updating this guide or its agent skills, walk these scenarios against the
revised guidance (read-only or in isolated fixtures):

| Reader task | Essential facts the resulting explanation must preserve |
|---|---|
| Switch an installed Git link to an overlay source | Exact-target declaration replacement differs from installed-link replacement; inspect and preserve the old destination, move only the intended link aside, repeat preview, and verify the new origin |
| Retire desktop links and return to base | Removal uses the original selection and copies current sources; base copies block link installation; preserve edits before clearing only the intended conflicts |
| Retry a partial service update | Select the failed run by stable ID, inspect predecessor failures, review dependency expansion, and explain that earlier changes remain |
| Document a task with no removal action | State what stops being managed and what persists; give no fictional remove command or rollback promise |
| Make a reference page discoverable without knowing its selector | Use a unique task-focused description and recognizable tool/platform terms; verify a natural-language query reaches the intended page and section |

Judge success by whether a real reader can complete the task and recognize the
outcome, not by hitting a rigid sentence or section count.

Once your code and docs are ready, continue to [Testing](testing.md) to select
and run your verification checks.
