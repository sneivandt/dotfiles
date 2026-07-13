# Agent Skill Index and Routing

Use this index to load only the skills needed for the current change. Start with
the narrowest matching skill. Companion skills are conditional: load one only
when its subsystem is also touched, and do not recursively load that skill's
companions. Avoid unrelated skills to reduce conflicting guidance.

## Skill index

| Skill | Use when | Conditional companions |
|---|---|---|
| [`ai-tooling-apm`](./skills/ai-tooling-apm/SKILL.md) | Changing Copilot/Codex/agent/APM config, local plugins, or APM-managed skill delivery | `config-validation`, `cross-platform-verification` |
| [`ci-cd-patterns`](./skills/ci-cd-patterns/SKILL.md) | Editing workflows, release/docker publishing, or CI reproduction | `cross-platform-verification`, `testing-patterns` |
| [`config-validation`](./skills/config-validation/SKILL.md) | Adding/changing config validators, test command validation tasks, or config drift tests | `toml-configuration`, `testing-patterns` |
| [`cross-platform-verification`](./skills/cross-platform-verification/SKILL.md) | Running the canonical local Rust/cross-platform verification sequence and wrapper checks | `windows-specific-patterns`, `shell-patterns` |
| [`engine-orchestration`](./skills/engine-orchestration/SKILL.md) | Task dependencies, scheduler behavior, process modes, operation workflows, or resource parallelism | `resource-implementation`, `logging-patterns`, `error-handling-patterns` |
| [`error-handling-patterns`](./skills/error-handling-patterns/SKILL.md) | Idempotency, dry-run, task results, and resource/task error handling | `logging-patterns` |
| [`git-hooks-patterns`](./skills/git-hooks-patterns/SKILL.md) | Hook installation, hook behavior, and sensitive-data scanning in `hooks/` | `shell-patterns`, `ci-cd-patterns` |
| [`logging-patterns`](./skills/logging-patterns/SKILL.md) | Logger usage, task recording, buffered output, or summary behavior | `testing-patterns` |
| [`module-organization`](./skills/module-organization/SKILL.md) | Moving/splitting/adding Rust modules under `cli/src/` | `rust-patterns`, `testing-patterns` |
| [`overlay-scripts`](./skills/overlay-scripts/SKILL.md) | Overlay config loading, convention-based script resources, or dynamic script tasks | `resource-implementation`, `profile-system`, `toml-configuration` |
| [`package-management`](./skills/package-management/SKILL.md) | Pacman/paru/winget package resource and task behavior | `resource-implementation`, `windows-specific-patterns` |
| [`profile-system`](./skills/profile-system/SKILL.md) | Profile resolution, category activation/exclusion, profile selection/persistence | `sparse-checkout-patterns`, `symlink-management`, `toml-configuration` |
| [`resource-implementation`](./skills/resource-implementation/SKILL.md) | Choosing and implementing `Resource`, `IntrinsicState`, or `ResourceStateProvider` | `error-handling-patterns`, `testing-patterns` |
| [`rust-patterns`](./skills/rust-patterns/SKILL.md) | Router for `cli/src/` changes and core Rust conventions | load the focused skill for the touched subsystem |
| [`shell-patterns`](./skills/shell-patterns/SKILL.md) | Wrapper/bootstrap shell behavior and POSIX hook script patterns | `windows-specific-patterns` |
| [`sparse-checkout-patterns`](./skills/sparse-checkout-patterns/SKILL.md) | `manifest.toml` category exclusions and sparse-checkout behavior | `toml-configuration` |
| [`symlink-management`](./skills/symlink-management/SKILL.md) | `conf/symlinks.toml`, symlink resource behavior, and manifest alignment | `config-validation` |
| [`testing-patterns`](./skills/testing-patterns/SKILL.md) | Test construction/layout/snapshots and test helper usage | none |
| [`toml-configuration`](./skills/toml-configuration/SKILL.md) | TOML section format, category filtering, and loader patterns for `conf/`, `app/config/`, and domain config modules | none |
| [`windows-specific-patterns`](./skills/windows-specific-patterns/SKILL.md) | Windows-only behavior (registry/symlink capability/platform details) | none |

## Task-to-skill routes

| Task | Read first | Then |
|---|---|---|
| Add/change config-backed resource behavior | `rust-patterns` | `resource-implementation`, `toml-configuration`, `config-validation`, `testing-patterns` |
| Change scheduling, dependencies, phases, or operation flow | `engine-orchestration` | `logging-patterns`, `error-handling-patterns`, `testing-patterns` |
| Change symlinks, profiles, or sparse checkout | `symlink-management` | `profile-system`, `sparse-checkout-patterns`, `toml-configuration`, `config-validation` |
| Change Windows behavior | `windows-specific-patterns` | `cross-platform-verification`, `testing-patterns`, `shell-patterns` |
| Change agent/APM configuration | `ai-tooling-apm` | `toml-configuration`, `config-validation`, `cross-platform-verification` |
| Add or update tests | `testing-patterns` | `cross-platform-verification`, `ci-cd-patterns` |
| Modify wrappers | `shell-patterns` | `cross-platform-verification`, `windows-specific-patterns` |
| Modify CI workflows/release publishing | `ci-cd-patterns` | `cross-platform-verification`, `testing-patterns` |
