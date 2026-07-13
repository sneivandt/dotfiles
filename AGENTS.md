# Dotfiles Agent Instructions

Start with the skill index: [`./.agents/README.md`](./.agents/README.md). Load
only relevant skills plus listed companions.

## Repository invariants

- Wrappers (`dotfiles.sh`, `dotfiles.ps1`) bootstrap/forward only.
- Declarative desired state lives in `conf/`.
- Independent config-backed state generally uses `Resource`.
- Whole-workflow convergence generally uses `Operation`.
- Tasks own metadata, policies, dependencies, and orchestration boundaries.
- Mutations must be idempotent and dry-run safe.
- Prefer capability methods over direct OS checks where available.
- Static install/uninstall tasks must be registered in
  `cli/src/app/catalog.rs`; command-specific tasks belong in their command's
  task list.
- Conditional symlink behavior and manifest coverage must stay synchronized.

## Entry matrix (pick layer, then skill)

| Change type | Read first | Then |
|---|---|---|
| Rust routing/conventions | `rust-patterns` | focused subsystem skill |
| Resources/providers | `resource-implementation` | `engine-orchestration`, `error-handling-patterns`, `testing-patterns` |
| Task dependencies/scheduling/operations | `engine-orchestration` | `logging-patterns`, `testing-patterns` |
| Profiles/sparse checkout/symlinks | `profile-system` / `sparse-checkout-patterns` / `symlink-management` | `toml-configuration`, `config-validation` |
| Windows-specific behavior | `windows-specific-patterns` | `cross-platform-verification`, `shell-patterns` |
| Agent/APM config | `ai-tooling-apm` | `toml-configuration`, `config-validation` |
| CI workflows/publishing | `ci-cd-patterns` | `cross-platform-verification`, `testing-patterns` |

## Standard change workflow

1. Identify the primary layer and routing skill.
2. Load only conditional companions whose subsystem is touched; do not recurse.
3. Find the closest existing implementation before editing.
4. Make the smallest complete vertical change (not a partial wiring).
5. Add/update focused tests.
6. Run targeted checks.
7. Review cross-platform and config-drift impact.
8. Explicitly report checks not run.

## Validation ownership

- Canonical general Rust/cross-platform sequence:
  `cross-platform-verification`
- Test construction/organization: `testing-patterns`
- CI workflow/publishing and CI-only reproduction: `ci-cd-patterns`
- Domain-specific checks remain in their owning skills (for example APM dry
  runs, config drift/validators, wrapper linting).

## Definition of done

- Full vertical slice wired (implementation and applicable config,
  registration/export).
- Tests added/updated where behavior changed.
- User-facing docs updated when behavior/workflow changed.
- Targeted validation run and passing.
- Checks not run are called out explicitly.
- No unrelated changes, unreviewed generated artifacts, private files, or
  secret-bearing changes.

## Vertical-slice checklists

### New config-backed resource

`config type -> loader -> validator -> conf file -> resource -> task -> command registration (catalog for install/uninstall) -> module exports -> tests -> cross-platform checks`

### New symlink

`source file -> conf/symlinks.toml -> conf/manifest.toml (if conditional) -> config drift coverage -> dry-run verification`

### New task

`task implementation -> metadata/domain/phase/policies -> dependencies -> command registration (catalog for install/uninstall) -> command/test coverage -> targeted validation`
