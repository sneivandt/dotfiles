---
name: arch-package-recipes
description: >
  Use for packages/*/PKGBUILD and release.json, custom-package-versions,
  custom-package-repository, disposable PKGBUILD snapshots, or Paru local
  repository cleanup. Not for ordinary conf/packages.toml entries, package
  providers alone, or APM manifests.
---

# Arch Package Recipes

See [Custom package versions](../../../docs/content/reference/custom-package-versions.md)
and [Custom package repository](../../../docs/content/reference/custom-package-repository.md)
for release descriptors, snapshots, and generated Paru config; implementation
lives in [arch_recipes.rs](../../../cli/src/domains/packages/arch_recipes.rs) and
[snapshots.rs](../../../cli/src/domains/packages/arch_recipes/snapshots.rs).

## Non-obvious invariants

- Track recipes and descriptors only—never generated version/checksum pins or
  build output, and do not add source-checkout ignore rules for misplaced builds.
- Validate all configured asset digests before publishing a recipe's pins;
  preserve failed recipes' existing pins and failure counts while independent
  work proceeds. Use the shared GitHub client (ETag revalidation and quota handling).
- Preview may query releases but never writes pins, caches, snapshots, or config,
  and never evaluates `PKGBUILD`s. Metadata validation executes recipes on
  disposable copies; those copies are not a sandbox.
- Cache successful validation against exact inputs, descriptor, and a versioned
  protocol; verify ownership, working files, and committed inputs before reuse,
  and repair stale/legacy metadata even when inputs and pins are unchanged.
- Keep owned snapshots aligned with `CloneDir`, repository `Path`, and actual
  `package_base`. Reject foreign repos, worktrees, and unsafe links. Commit
  supported inputs and validated `.SRCINFO` so Paru cleanup retains them; extend
  the documented format before adding new input types.
- Preserve ordering-only preparation edges and the blocking Paru-health edge.
  Omit unavailable custom recipes from publication/installation, count failures,
  and never fall back to the AUR.
- Keep bounded validation workers conditional on `ctx.parallel()`, with ordered
  publication, task log context, and cancellation propagation.

## Verification

Recipe-only changes must trigger offline Linux checks and PKGBUILD ShellCheck
without releasing the CLI. For cleanup changes, run build-only Paru cleanup in
an isolated home/cache with installation blocked and confirm source edits and
cached pins survive (a successful build/listing alone does not prove cleanup safety).
