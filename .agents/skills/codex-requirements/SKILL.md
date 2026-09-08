---
name: codex-requirements
description: >
  Use for the Linux task and resource that manage administrator-enforced Codex
  policy in /etc/codex/requirements.toml. Not for user Codex settings, APM
  packages, or symlinked agent configuration.
---

# Codex Requirements

## Ownership

| Concern | Owner |
|---|---|
| fixed managed policy | `cli/src/domains/ai/resources/codex_requirements.rs` |
| Linux task applicability and elevation planning | `cli/src/domains/ai/codex_requirements.rs` |
| install catalog membership | `cli/src/app/catalog.rs` |
| user-facing behavior | [`docs/TASKS.md`](../../../docs/TASKS.md#codex-requirements) |
| privilege boundary | [`docs/SECURITY.md`](../../../docs/SECURITY.md#elevation) |

This policy is not APM content or profile-selectable user configuration. Keep
it out of `symlinks/apm/`, `conf/agent-settings.toml`, and private overlays.

## Resource contract

- Merge managed tables recursively and preserve unrelated requirements.
- Reject malformed TOML and non-regular targets instead of replacing them.
- Keep constructors, state discovery, and dry runs read-only.
- On Linux, stage the merged document in a unique temporary file and install
  `/etc/codex/requirements.toml` as `root:root` mode `0644` through the injected
  executor.
- Request task elevation only when a missing or incorrect resource will be
  changed. Invalid state must remain visible as unmet work.

## Coverage

Keep policy, merge, state, command, failure-cleanup, and convergence cases with
the resource. Keep platform and CI applicability with the task, and catalog
selection in `cli/tests/install_command.rs`. Select validation from
[`docs/TESTING.md`](../../../docs/TESTING.md#choosing-coverage).
