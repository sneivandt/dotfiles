---
name: sparse-checkout-patterns
description: >
  Use for conf/manifest.toml ownership, sparse-checkout pattern generation, or
  profile-driven retention under cli/src/domains/repository/sparse_checkout*.
  Not for profile selection itself.
---

# Sparse Checkout Patterns

`conf/manifest.toml` maps paths relative to `symlinks/` to the category
combinations that own them.

## Ownership rules

- Section names use AND semantics: `[arch-desktop]` owns a path only when both
  categories are active.
- If multiple sections declare a path, any active owner retains it.
- Directory entries end with `/`.
- Base-owned paths need no manifest entry.
- A non-base symlink source reused by `[base]` is already retained.

The task writes cone-disabled patterns beginning with `/*` and exclusion entries,
then runs `git read-tree -mu HEAD`. Apply checkout changes only from a clean
working tree.

## Add or move a managed path

1. Determine its exact category ownership.
2. Update `conf/manifest.toml` unless base already owns the source.
3. Keep `conf/symlinks.toml` and real `symlinks/` content aligned.
4. Add or update config-drift coverage.

Use `profile-system` for category-set computation, `symlink-management` for
target behavior, and `config-validation` for drift invariants. Validation
coverage is listed under [Choosing coverage](../../../docs/TESTING.md#choosing-coverage).
