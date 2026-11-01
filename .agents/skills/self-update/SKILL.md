---
name: self-update
description: >
  Use for Rust binary self-update under cli/src/domains/dotfiles/self_update*,
  release selection/cache, download verification, binary replacement, smoke
  tests, and update restart integration. Not for Git repository synchronization,
  package upgrades, or release publishing alone.
---

# Self Update

## Find the boundary

| Concern | Start with |
|---|---|
| update applicability and orchestration | `cli/src/domains/dotfiles/self_update.rs` |
| release ordering and cache | `self_update/version.rs`, `self_update/cache.rs` |
| streaming download and checksum | `self_update/http.rs` |
| provenance policy and verification | `self_update/attestation.rs` |
| staged replacement, smoke test, recovery | `self_update/install.rs` |
| process restart | `cli/src/app/commands/` |

The abbreviated paths are relative to `cli/src/domains/dotfiles/`.
Wrappers own bootstrap before a binary exists; Rust owns normal binary
self-update. Git checkout synchronization is a separate workflow under
`cli/src/domains/repository/`.

## Preserve the trust and recovery sequence

- Preserve development-build/installed-path applicability, explicit skip policy,
  release ordering, and the distinction between offline discovery and failed
  installation. Do not treat arbitrary tags as newer releases.
- Verify the checksum and required provenance against the staged download
  before replacing the working binary. Preserve explicit attestation policy;
  do not silently weaken verification after a network or verification failure.
- Use the same staged bytes for verification and installation. Preserve unique
  staging, cleanup, executable permissions, and platform-specific replacement.
- Retain the old binary for recovery, smoke-test the installed binary with the
  supported version flag, and preserve both the original failure and any
  rollback failure. Do not report success merely because replacement succeeded.
- Preserve guarded restart and forwarded arguments; avoid restart loops or
  continuing with configuration/task inputs from the wrong process generation.

## Validate the changed boundary

Use the existing injected HTTP/provenance fixtures and temporary binary paths
in the owning modules. Cover failed download/checksum/provenance before
replacement, replacement failure, smoke-test failure, rollback, and successful
installation as relevant. Never replace the live `bin/dotfiles` to test a patch.
Use [Testing](../../../docs/content/testing.md#choosing-coverage) for commands and native
Windows coverage limits.

Add `shell-patterns` when changing wrapper parity, `ci-cd-patterns` when changing
release asset/tag/provenance production, and `engine-orchestration` only when
the restart or execution contract changes.
