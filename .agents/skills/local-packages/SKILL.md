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
  configured asset digest for a recipe before writing its pins. It does not
  install software. Preserve failed recipes' pins and failed-item accounting
  while allowing independent recipes to refresh; propagate cancellation.
  Preview may query the API but must not write release caches, pins, snapshots,
  or configuration. Release discovery uses the shared authenticated GitHub
  client in `infra/github.rs`, serializing requests and honoring cached metadata
  and quota cooldowns. Revalidate cached releases against current descriptors;
  stale metadata must not turn a failed refresh into success. PKGBUILD metadata
  validation uses bounded workers only when `ctx.parallel()` is enabled;
  preserve ordered publication and task log context.
- Cache only successful metadata validation, keyed by copied inputs, the release
  descriptor, and a versioned validation protocol. Recheck snapshot ownership,
  working files and committed inputs before reuse. Input damage, descriptor
  changes, or missing records require validation again; preview never writes
  validation records or evaluates PKGBUILDs.
- Paru builds from standalone, dotfiles-owned Git snapshots in its build cache,
  never from the source checkout. Snapshot HEAD must contain both recipe and
  pins so Git reset/clean preserves usable inputs. Do not bypass ownership,
  symlink, worktree, or external-directory checks to reuse a conflicting cache.
- Keep `CleanAfter` working. Align each snapshot with
  `<CloneDir>/<package_base>` and the generated repository `Path`; changing only
  the CLI invocation does not fix a user's standalone `paru -Syu`.
- Refresh, snapshot preparation, and AUR installation use ordering edges;
  Paru health remains a blocking prerequisite. Existing pins can prepare a
  snapshot after refresh failure. Omit unavailable recipes from generated
  repositories, and exclude their missing package names from direct installs
  while counting failures. Never fall back to the AUR for an unavailable local
  target. Verify committed inputs and the published repository section first.
- Package ownership defaults to `package_base`; use the optional `packages`
  descriptor array only for split packages or differing names. Invalid or
  missing ownership descriptors fail closed. Keep agent package tasks
  independent of the APM installation method.
- Snapshots copy `PKGBUILD`, `version.sh`, and optional `package.install`,
  `LICENSE`, and `NOTICE`. Tracked license/notice files need review when
  upstream terms change; version discovery does not refresh them.
  Use `install=package.install` for lifecycle hooks. Extend the format
  explicitly before introducing patches or other extra inputs.
- Asset names may use `{version}` for the validated release version without
  its leading `v`; fixed asset names continue to match literally.
- An optional `tag_prefix` selects an exact release tag prefix such as Codex's
  `rust-v`. A mismatch fails; absent prefixes retain optional leading-`v` handling.

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
