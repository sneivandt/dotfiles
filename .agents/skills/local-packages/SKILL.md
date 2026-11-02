---
name: local-packages
description: >
  Use for packages/*/PKGBUILD and release.json, local-package-versions,
  local-package-repository, disposable PKGBUILD snapshots, or Paru local repository cleanup.
  Not for ordinary conf/packages.toml entries or package providers alone, or
  APM agent dependency/deployment manifests.
---

# Local Packages

Read [Local package recipes](../../../docs/content/local-package-recipes.md) for the lifecycle,
descriptor format, cache ownership, Paru paths, and adding recipes.

## Find the owner

| Change | Start with |
|---|---|
| names, dependencies, URLs, packaging | `packages/<recipe>/PKGBUILD` |
| release source, architecture assets, ownership | `packages/<recipe>/release.json` |
| discovery, pins, generated Paru config | [local_builds.rs](../../../cli/src/domains/packages/local_builds.rs) |
| cached snapshots and cleanup safety | [snapshots.rs](../../../cli/src/domains/packages/local_builds/snapshots.rs) |
| shared Paru options | `symlinks/config/paru/paru.conf.base` |
| automatic installation selection | `conf/packages.toml`; use `package-management` for providers |

## Preserve these contracts

- Track recipes/descriptors, not generated version/checksum pins or build output.
  Do not add source-checkout ignore rules to accommodate building in the wrong
  location. Review tracked license/notice changes separately from version refresh.
- Validate all configured asset digests before publishing a recipe's pins;
  preserve failed recipes' pins and failure counts while independent work proceeds.
  Use the shared GitHub client and preserve cache revalidation and quota handling.
- Preview may inspect/query but never writes pins, caches, snapshots, or config,
  and never evaluates PKGBUILDs. Metadata validation executes reviewed recipes
  on disposable copies; those copies are not a sandbox.
- Cache successful validation against exact inputs, descriptor, and a versioned
  protocol. Check ownership, working files, and committed inputs before reuse.
  Repair stale/legacy metadata even when recipe inputs and pins are unchanged.
- Keep standalone owned snapshots aligned with `CloneDir`, repository `Path`,
  and actual `package_base`. Reject foreign repositories, worktrees, and unsafe
  links. Commit supported inputs and validated `.SRCINFO` so Paru cleanup retains
  them. Extend the documented format before adding patches or other inputs.
- Preserve ordering-only preparation edges and the blocking Paru-health edge.
  Retained pins can remain useful after discovery failure. Omit unavailable local
  recipes from publication/installation, count failures, and never fall back to
  the AUR. Verify current inputs and published sections before direct install.
- Keep bounded validation workers conditional on `ctx.parallel()`, ordered
  publication, task log context, and cancellation propagation. Agent deployment
  must stay independent of the APM installation method.

## Verify the changed contract

Use [Testing](../../../docs/content/testing.md), the
[CI coverage](../../../docs/content/local-package-recipes.md#ci-and-validation), and focused
[regressions](../../../cli/src/domains/packages/local_builds/tests.rs).
Recipe-only changes must select offline Linux checks and PKGBUILD ShellCheck
without releasing the CLI solely because a recipe changed; use `ci-cd-patterns`
for routing changes.

For cleanup changes, exercise build-only Paru cleanup in an isolated home/cache
with installation blocked, and confirm source edits and cached pins survive.
A successful build/listing alone does not prove cleanup safety; offline checks
cannot establish native installation or upstream availability.
