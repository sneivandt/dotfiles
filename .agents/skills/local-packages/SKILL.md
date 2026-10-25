---
name: local-packages
description: >
  Use for packages/*/PKGBUILD and release.json, local-package-versions,
  paru-config, disposable PKGBUILD snapshots, or Paru local repository cleanup.
  Not for ordinary conf/packages.toml entries or package providers alone, or
  APM agent dependency/deployment manifests.
---

# Local Packages

Read [Local package recipes](../../../docs/LOCAL-PACKAGES.md) for the workflow,
file ownership, descriptor example, adding recipes, logs, and CI limits.

## Find the owner

| Change | Start here |
|---|---|
| Package name, dependencies, archive URLs, or packaging steps | `packages/<recipe>/PKGBUILD` |
| GitHub repository, architecture assets, or package base | `packages/<recipe>/release.json` |
| Release discovery, pin generation, or Paru config | [local_builds.rs](../../../cli/src/domains/packages/local_builds.rs) |
| Cached Git snapshots and cleanup safety | [snapshots.rs](../../../cli/src/domains/packages/local_builds/snapshots.rs) |
| General Paru options | `symlinks/config/paru/paru.conf.base` |
| Automatic installation selection | `conf/packages.toml`; use `package-management` for provider behavior |

## Preserve the boundaries

- Track recipes and release-source descriptors. Keep discovered versions and
  checksums out of the dotfiles Git history; generated `version.sh` and build
  output belong in the root `.gitignore`.
- `package_base` must match makepkg's actual package base; the source directory
  name need not match it. Use a distinct local package name where appropriate
  so an AUR recipe does not replace the maintained recipe.
- The version task queries stable GitHub release metadata and validates every
  configured asset digest before writing pins. It does not install software.
  Preserve errors and existing pins on discovery failure; preview may query
  the API but must not write pins, snapshots, or configuration.
- Paru builds from standalone, dotfiles-owned Git snapshots in its build cache,
  never from the source checkout. Snapshot HEAD must contain both recipe and
  pins so Git reset/clean preserves usable inputs. Do not bypass ownership,
  symlink, worktree, or external-directory checks to reuse a conflicting cache.
- Keep `CleanAfter` working. Align each snapshot with
  `<CloneDir>/<package_base>` and the generated repository `Path`; changing only
  the CLI invocation does not fix a user's standalone `paru -Syu`.
- Snapshot preparation requires successful pin discovery before AUR installs.
  Keep agent package tasks independent of the APM installation method.
- The current snapshot format copies only `PKGBUILD` and `version.sh`. Extend
  it explicitly before introducing patches, install scripts, or extra inputs.

## Verify the changed contract

Use [Testing](../../../docs/TESTING.md) and the
[local-package CI coverage](../../../docs/LOCAL-PACKAGES.md#ci-and-validation).
The focused regressions are in
[tests.rs](../../../cli/src/domains/packages/local_builds/tests.rs).

Recipe paths must select their offline Linux checks and PKGBUILD ShellCheck,
without triggering a CLI release solely because a recipe changed. Use
`ci-cd-patterns` when changing that routing or workflow conditions.

For cleanup changes, a successful makepkg build or repository listing alone is
insufficient: exercise build-only Paru cleanup in an isolated home/cache with
installation commands blocked, and confirm source edits and cached pins survive.
Offline CI does not establish native Arch installation or upstream availability.
