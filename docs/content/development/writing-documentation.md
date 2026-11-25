---
title: Writing documentation
description: Choose a guide's scope, verify procedures and examples, write searchable page metadata, and maintain task references for human readers.
---

# Writing documentation

Start with the question that brought someone to the page. A first-time user
needs to know what a preview will do; someone looking up `--only` needs its
scope and syntax. Answer that question first. Give enough context to act and
a way to tell whether it worked.

For previews of the site itself, build checks, and publication, use the
[site maintenance README](https://github.com/sneivandt/dotfiles/blob/main/docs/README.md).

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

A guide walks the reader through a job; a reference answers a lookup. Give the
full explanation one home and link to it from the other. Repeat a consequence
where it changes the reader's decision, not a disclaimer on every page. For
example, the overlay guide owns script selection, but anyone about to preview
an untrusted overlay still needs to know that the scripts execute.

## Verify behavior and transitions

Before writing, read the owning command or task, its resource, its config
decoder, and its tests. For task eligibility and dependencies, also check
[app/catalog.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/app/catalog.rs)
or the `check` command's task list. Never rely solely on older docs or an
upstream tool's manual to assume what dotfiles supports.

Trace the procedure from the reader's starting point: a fresh machine, an
existing installation, or a partially failed run. Three questions catch errors
that valid command syntax won't:

1. **What does the CLI know?** A task that doesn't apply is different from
   one that lacks a tool. A failed inspection doesn't mean the file is absent.
2. **What changes at this step?** A preview isn't a promise that apply will
   succeed. It can also have effects of its own, such as saving a profile or
   overlay path. Changing a declaration doesn't retarget an installed link.
3. **What's left for the next step?** For instance, removal can leave a copy
   that blocks a new link. Explain what to preserve, how to identify the
   intended destination, and how to tell when the old source is no longer needed.

Check the normal path and a realistic recovery path against code or an
isolated fixture, not your live home directory or package manager. Link to an
upstream manual for the tool's own behavior, to tracked source for an
implementation detail, and to a human guide rather than an agent skill for
instructions.

## Make examples reusable

Choose small examples that illustrate one decision at a time: familiar package
names, a simple Git setting, or a clearly fictional app usually teach faster
than a custom recipe or machine-specific unit.

Tell readers whether they are looking at a real excerpt (link its source), a
working example, or a sketch that omits setup. Name any placeholder they must
replace and any tools or source files the example needs.

For configuration snippets, state which file they belong in and whether to
extend an existing array rather than duplicating a TOML table header or
replacing a whole file.

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
tool requirements, and dependencies. Keep cells short. Put the explanation
of an unusual ordering rule or failure mode in the body rather than making
the reader piece it together across columns.

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

Lead with the common path, using direct sentences and task-oriented headings.
Explain *why* a choice matters in terms of an observable effect. For example,
“Keep the checkout: your installed links point into it” is more useful than
“A permanent checkout is required.” Use the same term across guides and
reference pages; don't alternate between “role,” “tier,” and “mode” to avoid
repeating “profile.”

Use a table for short values people compare. If each row reads like a
paragraph, try a list or short subsections instead. Use a diagram when a
relationship is easier to see than to describe, such as a home link pointing
into the checkout. Prefer a top-to-bottom flow with short labels, give it an
accessible title and description, and keep the essential point in nearby
prose. Check the result at phone width: contained scrolling can prevent page
overflow without making a diagram or table easy to read.

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
widths. Read the page once as a direct link and once as the next step from a
neighboring guide. A direct visitor shouldn't miss a prerequisite, and a
reader following the sequence shouldn't get the same introduction twice.

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
