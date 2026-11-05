---
name: cross-platform-verification
description: >
  Use to select validation after Rust source/tests or Cargo build changes in
  cli/, or changes to dotfiles.sh or dotfiles.ps1. Covers Linux/Windows checks
  and native-runtime gaps; not implementation patterns or documentation-only work.
---

# Cross-Platform Verification

1. Select checks from [Choosing coverage](../../../docs/content/testing.md#choosing-coverage)
   and use the [local stages](../../../docs/content/testing.md#fast-local-sequence)
   for exact commands. Group affected test targets; broaden for shared engine,
   catalog, or loader changes and failures that reveal wider impact.
2. For platform-sensitive changes, consult
   [Platform coverage parity](../../../docs/content/testing.md#platform-coverage-parity).
   Cross-target compilation cannot prove native registry, elevation, symlink,
   or Windows path behavior. Report the remaining native-runtime gap.
3. Use isolated fixtures and the current source with the pinned toolchain.
   Do not substitute a host install or a bootstrap-downloaded binary for tests.
   A skipped stage is unavailable coverage, even if the runner exits successfully.
4. Use focused tests during editing, then one committed pre-push gate.
   On Windows, the checker runs native fixtures and coordinates Linux checks
   through WSL with separate Linux snapshots and Cargo artifacts. Native tests
   on two operating systems are complementary, not redundant. Do not add a
   Windows cross-compile when native Windows checks already cover that commit.
5. Documentation needs one successful native-platform check, not platform
   parity. The coordinator avoids repeating it in WSL. Reuse a separate
   successful committed docs result through `--docs-checked` only for the same
   commit. Do not substitute earlier working-tree checks for the final gate.
