---
name: cross-platform-verification
description: >
  Use to select validation after Rust source/tests or Cargo build changes in
  cli/, or changes to dotfiles.sh or dotfiles.ps1. Covers Linux/Windows checks
  and native-runtime gaps; not implementation patterns or documentation-only work.
---

# Cross-Platform Verification

1. Select checks from [Choosing coverage](../../../docs/TESTING.md#choosing-coverage)
   and use the [local stages](../../../docs/TESTING.md#fast-local-sequence)
   for exact commands. Group affected test targets; broaden for shared engine,
   catalog, or loader changes and failures that reveal wider impact.
2. For platform-sensitive changes, consult
   [Platform coverage parity](../../../docs/TESTING.md#platform-coverage-parity).
   Cross-target compilation cannot prove native registry, elevation, symlink,
   or Windows path behavior. Report the remaining native-runtime gap.
3. Use isolated fixtures and the current source with the pinned toolchain.
   Do not substitute a host install or a bootstrap-downloaded binary for tests.
   A skipped stage is unavailable coverage, even if the runner exits successfully.
