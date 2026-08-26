---
name: package-management
description: >
  Use for conf/packages.toml or package providers/resources under
  cli/src/domains/packages/, including pacman, AUR helpers, and winget. Not for
  generic subprocess or task orchestration changes.
---

# Package Management

## Architecture

- Config owns exact package identifiers and explicit metadata such as `aur`.
- `PackageManager` selects a provider.
- Provider modules own query/install commands.
- Package resources own state and mutation.
- The install task/operation owns applicability and batching.

Add a provider rather than branching manager-specific commands through tasks.

## Convergence

1. Select entries and provider from platform capabilities.
2. Verify the executable exists.
3. Query installed state once per provider.
4. Plan missing entries from that cached state.
5. Batch installs where the provider supports it.

Route every command through the executor and preserve provider-specific exact,
noninteractive, and already-present behavior. An unavailable manager returns an
explicit skip or diagnostic, never silent success.

## Platform rules

- Pacman handles ordinary Arch packages; AUR bootstrap and AUR packages stay
  separate.
- Do not wrap AUR helpers in an extra sudo layer.
- Winget uses exact IDs, prefers user scope, and retries unscoped only when no
  user-scope installer exists. Privilege-only failures are explicit skips.

Test provider commands, state mapping, batching, missing-manager behavior, and
dry-run planning. Use `toml-configuration` if entry syntax changes and
[Testing](../../../docs/TESTING.md) for commands.
