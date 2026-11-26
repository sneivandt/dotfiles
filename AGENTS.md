# Dotfiles Agent Instructions

This repository manages Linux and Windows desired state through a Rust CLI.
Start with the files that own the requested behavior; application configuration
changes usually do not need engine changes.

Before proposing a change in behavior, check the
[design decisions](docs/content/development/architecture.md#design-decisions).
If you recommend revisiting one, explain what problem the current choice causes
and what would be lost by changing it.

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
- Keep Rust modules and dependencies lean: avoid directories containing only a
  single `.rs` file unless a platform or test split genuinely calls for it,
  split oversized modules and test suites by responsibility into cohesive
  sibling files, and avoid niche or lightly used crates in `cli/Cargo.toml`
  when small focused code suffices.
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

- Update the owning guide when a change affects what readers need to know or do.
  Follow [Writing documentation](docs/content/development/writing-documentation.md)
  for prose conventions and acceptance scenarios, and use
  [docs-writing](.agents/skills/docs-writing/SKILL.md) or
  [docs-publishing](.agents/skills/docs-publishing/SKILL.md) as needed.

## Before pushing

- An unqualified request to push from a `main` checkout means a direct push to
  `origin/main`. Follow any explicitly requested branch or pull request workflow.
  Preserve unrelated working-tree changes in a shared checkout by staging
  explicit task paths, refresh the destination, and verify that every outgoing
  commit belongs to the task before pushing.
- Choose the smallest useful local checks for the committed diff using
  [Testing](docs/content/development/testing.md#choosing-coverage) (and
  [docs/README.md](docs/README.md#checks) for docs/skills, plus
  [focused local mutations](docs/content/development/testing.md#focused-mutation-testing)
  for risky safety/dry-run/elevation changes). Explain what ran locally and what
  is deferred to CI; never treat deferred or skipped checks as local passes.
- Before full validation or a large Rust build in a worktree, check the
  [validation cache](docs/content/development/testing.md#validation-cache).
  Get explicit authorization before deleting a worktree, toolchain, or saved
  agent artifact.
- A request to push includes monitoring the exact commit's aggregate CI to
  completion (see [CI monitoring](.agents/skills/ci-cd-patterns/SKILL.md#after-pushing),
  and verify publication when reporting a release). Do not launch or wait for
  nightly mutation testing on ordinary pushes. If monitoring is blocked, report
  the exact SHA, run, and blocker instead of claiming success.
