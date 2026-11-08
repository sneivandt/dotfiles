# Dotfiles Agent Instructions

This repository manages Linux and Windows desired state through a Rust CLI.
Start with the files that own the requested behavior; application configuration
changes usually do not need engine changes.

## Working safely

- This checkout may be the live source of home-directory symlinks. Edit tracked
  sources, not installed copies, and account for applications that auto-reload.
- A code change is not permission to run `dotfiles update` or `dotfiles remove`
  against the host, update packages, deploy agent plugins, restart the desktop,
  or make privileged changes. Use isolated fixtures and scoped checks. Ask before
  machine changes outside the user's authorized scope.
- Never include private overlay content, credentials, or unsanitized machine logs
  in public code, fixtures, skills, or remote requests.
- Dry-run is not a sandbox: it can inspect the host, write logs and persist
  selection metadata; overlay preview scripts execute code. Use fixtures to
  verify that managed state is unchanged.

## Implementation boundaries

- Keep desired-state declarations in `conf/` and managed content in `symlinks/`
  or `system/`. Wrappers bootstrap and forward; Rust owns command semantics.
- Use `Resource` for independent items and `Operation` for a workflow that
  converges as one unit. Reuse their lifecycle helpers; keep discovery read-only,
  apply idempotent and preview non-mutating. Failed discovery is not absence.
- Tasks own metadata, applicability and dependencies. Register static
  `update`/`remove` tasks in [`app/catalog.rs`](cli/src/app/catalog.rs);
  command-specific tasks belong in that command's task list. Cross-domain
  dependencies belong in the app layer; list order does not schedule tasks.
- Use context capabilities and injected adapters for runtime platform decisions.
  Keep compile-time gates where platform-only APIs require them.
- Keep conditional symlinks aligned with profile categories. Removing a profile
  category does not authorize deleting previously managed state; `remove` is
  a separate capability, not a universal undo.

## Human documentation

- For each change, decide whether readers need to know or do something
  differently. Update the owning guide when it helps them. Use
  [docs-writing](.agents/skills/docs-writing/SKILL.md) for clear, accurate,
  natural prose at the right level of detail. Use
  [docs-publishing](.agents/skills/docs-publishing/SKILL.md) for documentation
  organization, README/site links, navigation, and publishing.
- Keep one clear explanation per topic, short commands using real defaults, and
  matching page titles, filenames, and URLs. Update references when moving content;
  do not add rename-compatibility routes.
- Review consequential procedures from their actual starting state, including
  already-installed configuration and partial failure. Use
  [Writing documentation](docs/content/development/writing-documentation.md) for content
  conventions and acceptance scenarios; skills route work rather than carrying
  a second copy of human procedures.

## Before pushing

- Inspect the full intended diff, including renames/deletions and pending changes
  from failed pushes. Choose local checks by affected behavior and test inputs
  using [Testing](docs/content/development/testing.md#choosing-coverage). The agent owns this
  decision; briefly explain the selected coverage and what remains for CI.
- Run the smallest useful checks with the pinned toolchain. Broaden for shared
  behavior, uncertain impact, or failures. Do not run unrelated suites or repeat
  passing checks unless relevant inputs changed. Keep commit hooks lightweight.
- For skills and prose, check metadata and links as affected. Use
  [docs/README.md](docs/README.md#checks) to decide whether a site build or browser
  review adds evidence; neither is automatic for Markdown or mixed patches.
- Validate the committed patch. Focused checks are sufficient when they cover
  the affected behavior; the [pre-push coordinator](docs/content/development/testing.md#before-pushing)
  is an optional conservative tool, not a mandatory second gate. Add cross-target
  CLI compilation only when the diff can affect that target. Native runtime
  checks cover behavior that cross-compilation cannot; remote CI can supply them.
- Fix selected local failures. Distinguish intentionally deferred CI coverage
  from checks that could not run; neither is a local pass. Do not provision a
  second OS or build the docs site solely to duplicate remote CI.
- Use [focused mutation testing](docs/content/development/testing.md#focused-mutation-testing)
  locally when it helps verify changed safety or failure behavior, or investigate
  a survivor; otherwise rely on the selected remote shards. When fixing a
  survivor, rerun the original source range even for a test-only fix.
- A request to push includes monitoring CI by default, without a separate
  request. After an authorized push, monitor the exact commit's aggregate CI and inspect
  selected mutation shard reports. Survivors, timeouts, and tool failures block
  CI. Follow the [CI monitoring guidance](.agents/skills/ci-cd-patterns/SKILL.md#after-pushing)
  and verify publication when reporting a release. Do not report the push task
  complete while its checks are pending. If monitoring is blocked, report the
  exact SHA, run, and blocker instead of claiming success.
