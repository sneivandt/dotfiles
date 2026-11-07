---
name: cross-platform-verification
description: >
  Use to select validation after Rust source/tests or Cargo build changes in
  cli/, or changes to dotfiles.sh or dotfiles.ps1. Covers Linux/Windows checks
  and native-runtime gaps; not implementation patterns or documentation-only work.
---

# Cross-Platform Verification

1. Inspect the complete diff and affected consumers, tests, fixtures, embedded
   inputs, and platform gates. Select the smallest useful checks from
   [Choosing coverage](../../../docs/content/testing.md#choosing-coverage).
   Briefly explain local coverage and what native CI will establish.
2. Rust source changes normally need fmt, host Clippy, and affected test targets.
   Group targets in one invocation. Broaden for shared engine, catalog, loader,
   or failure-policy changes, uncertain dependencies, and failures. Test-only
   edits need their owning targets; they do not automatically need full builds,
   minimum-Rust checks, or unrelated application suites.
3. Add cross-target compilation when changed gates, imports, types, dependencies,
   or shared APIs can affect the other target. An isolated Linux implementation
   edit need not compile Windows. Shared portable code can affect both platforms.
   Use [Platform coverage parity](../../../docs/content/testing.md#platform-coverage-parity)
   for native-runtime limits: cross-compilation cannot verify Windows paths,
   registry, elevation, process, or symlink behavior. Native exact-commit CI can
   supply this coverage; do not provision another OS just to duplicate it.
4. Use isolated fixtures and current source with the pinned toolchain. Never
   substitute host installation or a bootstrap-downloaded binary for tests.
   Distinguish a failed check, unavailable coverage, and deliberate CI deferral.
   Fix local failures; report gaps without calling them passes.
5. Focused checks can satisfy local pre-push validation. Confirm their tested
   inputs match the committed patch; repeat only after relevant changes or
   uncertain provenance. The coordinator is an optional conservative tool;
   inspect its plan because CI path selection is broader than local needs.
   Do not run it automatically after the same focused checks.
6. A CLI-only diff needs no local docs build. For mixed patches select docs
   checks separately from [docs/README.md](../../../docs/README.md#checks).
   Metadata and links generally suffice for prose and skills. Build or run
   browsers only for affected site behavior; one native platform is enough.
7. After pushing, follow [CI monitoring](../ci-cd-patterns/SKILL.md#after-pushing).
   Local selectivity does not narrow remote gates or mutation-report review.
