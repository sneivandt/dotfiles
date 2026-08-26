---
name: cross-platform-verification
description: >
  Use after changes to cli/src/, dotfiles.sh, or dotfiles.ps1 to select the
  canonical Linux/Windows checks. This is validation guidance, not an
  implementation-pattern skill.
---

# Cross-Platform Verification

Follow the canonical [fast local sequence](../../../docs/TESTING.md#fast-local-sequence)
and choose focused checks from
[Choosing coverage](../../../docs/TESTING.md#choosing-coverage). The
[platform coverage matrix](../../../docs/TESTING.md#platform-coverage-parity)
owns cross-target commands and records which behavior requires native CI.

Cross-target compilation does not prove Windows runtime behavior. Retain Windows
CI or VM coverage for registry, elevation, symlinks, and path handling, and
report unavailable target toolchains.

Typical failures are stale `cfg` gates, platform-only imports in shared code,
hardcoded separators, and executable-suffix assumptions.
