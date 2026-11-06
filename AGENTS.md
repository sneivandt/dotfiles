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
  [Writing documentation](docs/content/writing-documentation.md) for content
  conventions and acceptance scenarios; skills route work rather than carrying
  a second copy of human procedures.

## Before pushing

- Use focused checks while editing and lightweight commit hooks. Run broad
  coverage once at the final committed gate, not again at every layer.
- Choose coverage from [Testing](docs/content/testing.md#choosing-coverage), including
  affected script/application regressions. Use the pinned Rust toolchain.
- Use [docs/README.md](docs/README.md#checks) for focused documentation commands
  and its final native gate. One native documentation result is enough,
  including mixed patches; cross-platform parity applies to CLI behavior.
- Validate the committed patch with the [pre-push checker](docs/content/testing.md#before-pushing).
  A successful commit hook or focused test alone is not push validation.
  On Windows, use its coordinated native Windows and WSL checks. Do not treat
  cross-compilation as a substitute for available native runtime tests.
- Fix selected local failures. Report unavailable checks and remaining Windows,
  integration, and mutation coverage explicitly; a skip is not a pass.
  A confirmed local npm TLS failure may use the
  [explicit unavailable-coverage receipt](docs/content/testing.md#local-npm-tls-failures).
  Continue the other local checks and require the exact commit's CI documentation
  job to pass; do not disable TLS validation or report unavailable checks as passed.
- Run [focused mutation testing](docs/content/testing.md#focused-mutation-testing)
  for changed Rust behavior, especially safety checks, scheduling, previews, and
  partial failures. When fixing a survivor, rerun the original source range
  even if the fix changes only tests.
- A request to push includes monitoring CI by default, without a separate
  request. After an authorized push, monitor the exact commit's aggregate CI and inspect
  selected mutation shard reports. Survivors, timeouts, and tool failures block
  CI. Follow the [CI monitoring guidance](.agents/skills/ci-cd-patterns/SKILL.md#after-pushing)
  and verify publication when reporting a release. Do not report the push task
  complete while its checks are pending. If monitoring is blocked, report the
  exact SHA, run, and blocker instead of claiming success.
