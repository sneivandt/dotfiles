---
name: cross-platform-verification
description: >
  Use to select validation after Rust source/tests or Cargo build changes in
  cli/, or changes to dotfiles.sh or dotfiles.ps1. Covers Linux/Windows checks
  and native-runtime gaps; not implementation patterns or documentation-only work.
---

# Cross-Platform Verification

## Choose the smallest sufficient check

1. Classify the changed behavior using
   [Choosing coverage](../../../docs/TESTING.md#choosing-coverage).
2. Run the affected test targets together with the same runner/profile. Add
   formatting and compilation/lint checks for changed Rust or wrapper code.
3. Widen to the full sequence for shared engine, catalog, or config-loader
   changes, or when focused failures reveal broader impact. Do not run every
   stage by default for a local change.
4. Use the [platform coverage matrix](../../../docs/TESTING.md#platform-coverage-parity)
   for Windows-sensitive compilation and native-runtime coverage.

The [local check scripts](../../../docs/TESTING.md#fast-local-sequence) own stage
commands. A stage marked `SKIP` is not a pass, even if the overall script exits
successfully. Report material coverage gaps rather than claiming parity.

Cross-target compilation does not prove Windows runtime behavior. Retain Windows
CI or VM coverage for registry, elevation, symlinks, and path handling, and
report unavailable target toolchains.

Typical failures are stale `cfg` gates, platform-only imports in shared code,
hardcoded separators, and executable-suffix assumptions.

Use fixture-based integration checks; do not substitute a real machine install
for missing test coverage. Avoid bootstrap downloads or running an old installed
binary when the behavior under test is in the current Rust source.
