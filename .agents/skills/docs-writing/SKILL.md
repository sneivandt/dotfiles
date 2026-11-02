---
name: docs-writing
description: >
  Use when drafting, auditing, or editing human-facing prose in README.md,
  docs/content/, or application READMEs: clarity, factual accuracy, natural
  language, examples, and the right level of detail. Not for site navigation,
  rendering, or publishing alone (docs-publishing), or internal agent
  instructions (skill-maintenance).
---

# Documentation Writing

Write for the person doing the work, not to sound like a documentation generator.
Use [Contributing](../../../docs/content/contributing.md#documentation) for topic
ownership and [Testing](../../../docs/content/testing.md#documentation-site) for
checks. Load [docs-publishing](../docs-publishing/SKILL.md) when changing page
organization, routes, navigation, rendering, or publication.

## Establish what the reader needs

1. Identify the reader, their task, and the next decision or action. Put the
   common workflow before inventories, internals, and exceptions.
2. Verify commands, defaults, paths, prerequisites, and behavioral claims against
   the owning configuration, code, or tests. Existing prose is not proof.
   State uncertainty or coverage limits rather than filling gaps with assumptions.
3. Keep the detail needed to act correctly. Explain unfamiliar terms on first
   use, but do not teach every underlying tool. Put algorithms and rare
   implementation cases in the owning maintenance reference.
4. Give each explanation one home and link to it. Do not duplicate setup steps
   across landing pages and platform guides, or turn the README into a second
   documentation directory. Split pages by reader task, not length alone.

## Use natural language and punctuation

- Use direct sentences, concrete nouns, and familiar verbs. Prefer "applies the
  settings" to "converges the desired configuration" in user instructions.
  Keep technical terms when precision requires them.
- Do not use em dashes in authored prose. Use a period, comma, colon, or
  parentheses where the sentence calls for it. Do not substitute spaced hyphens
  or double hyphens as decorative sentence breaks.
- Prefer ordinary punctuation. Avoid ellipses in headings, decorative arrows,
  stacked parenthetical asides, and long semicolon chains. Do not change exact
  commands, filenames, quoted output, code operators, or meaningful symbols
  merely to enforce a prose preference.
- Avoid canned openings, promotional claims, and filler such as "let's dive in",
  "seamlessly", "robust solution", or "it's important to note". Say what happens,
  what the reader should do, and why the distinction matters.
- Vary sentence length naturally. Do not force every paragraph into the same
  formula or end every section with a recap. Remove sentences that only repeat
  a heading or announce what the next paragraph will explain.
- Use headings, bold, tables, and lists to help readers find information, not
  to decorate every sentence. Use numbered steps for an actual sequence, tables
  for comparisons, and prose for explanation. Avoid nested lists and excessive
  one-line sections.

## Make examples usable and honest

- Use short, copyable commands with verified defaults. Keep flags that express
  the purpose, such as a targeted preview. Avoid routine root/profile overrides
  and diagnostic flags unless the example needs them.
- State the working directory and shell when they matter. Label excerpts and
  placeholders; do not present invented paths or IDs as real defaults.
- Explain consequential effects next to the action: replacement without backup,
  package upgrades, scheduled deletion, elevation, or code execution. A preview
  is not a sandbox, and successful checks do not prove full runtime coverage.
- Link tracked sources where the reader needs to inspect or edit them. Distinguish
  source files from installed links and generated output.
- Use public, synthetic examples. Never include private overlay content,
  credentials, or unsanitized logs.

## Review the finished text

Read the page in order as its intended reader. Can they find the first action,
understand what it changes, and recognize success or failure? Remove duplication
and unnecessary detail without deleting safety information or exceptions needed
for the task. Check links after changing headings or paths. Re-read punctuation
and wording for a natural voice; automated link and site checks cannot assess
clarity, factual accuracy, or whether the amount of detail is appropriate.
