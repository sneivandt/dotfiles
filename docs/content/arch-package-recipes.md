---
title: Arch package recipes
description: Maintain release descriptors and PKGBUILDs, prepare isolated snapshots for Paru, and verify recipe changes.
---

# Arch package recipes

This guide is for maintainers adding or changing the custom recipes under
[packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/).
To select an existing package, install it, or upgrade it, use
[Package management](package-management.md#custom-arch-packages).

## Recipe inputs

Each recipe has a tracked `PKGBUILD` and `release.json`. Use an
[existing recipe](https://github.com/sneivandt/dotfiles/tree/main/packages/)
as the format reference. The descriptor identifies a stable GitHub release,
package base/names, and assets with published SHA-256 digests for every configured
architecture. The PKGBUILD sources generated `version.sh` pins; do not edit or
commit those generated files. Bump tracked `pkgrel` for packaging-only revisions.

Keep the descriptor's package base, names, and architectures consistent with
makepkg. Supported snapshot inputs are `PKGBUILD`, `version.sh`, optional
`package.install`, `LICENSE`, `NOTICE`, and generated `.SRCINFO`. Other local
inputs require extending the snapshot implementation first.

Adding a recipe selects it for discovery and preparation on Arch, independently
of package declarations. Add an `aur = true` declaration separately when a
profile should install it.

## Prepare snapshots

::: warning Review recipes before preparing packages
PKGBUILDs execute shell code during metadata generation, even without building
or installing. Temporary copies are not a sandbox. Review sources before
applying preparation; Paru skips normal recipe review for these local repositories.
:::

::: warning Preparation replaces Paru preferences
Preparation replaces the complete Paru configuration with a document
generated from
[paru.conf.base](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/paru/paru.conf.base).
Back up existing Paru preferences and incorporate the ones you want into that
tracked source before applying. Existing destination contents are not merged.
The [Custom package repository reference](custom-package-repository.md)
explains destination overrides and snapshot ownership.
:::

On an Arch machine with a configured checkout and selected profile, preview
preparation without installing applications:

```sh
dotfiles update --only custom-package-versions,custom-package-repository --dry-run
```

Check the proposed release pins, snapshot paths, and Paru configuration
destination. Remove `--dry-run` after review to refresh pins and prepare snapshots.
This scoped run needs GitHub metadata access and makepkg when applying; it
does not install missing tools. Preview does not execute recipes or write pins,
snapshots, caches, or Paru configuration. The shared
[preview limits](commands-and-usage.md#dry-run-scope) still apply.

Snapshots are separate, owned Git repositories under Paru's build cache so
`CleanAfter` cannot reset the source checkout. Keep `CloneDir`, repository `Path`,
and the descriptor's package base aligned. Use the same environment for dotfiles
and Paru. Edit tracked sources, not cached inputs or generated Paru sections.
Preparing a snapshot does not upgrade the application; follow the
[Arch upgrade workflow](package-management.md#arch-linux) when appropriate.

## Diagnose preparation

Start with the `custom-package-versions` or `custom-package-repository` log and
the [package troubleshooting table](package-management.md#logs-and-troubleshooting).
A failed refresh keeps previous pins where possible but still fails the task.
Investigate snapshot ownership conflicts without deleting unfamiliar caches
or forcing a remote AUR fallback.

Implementation details and regression cases live in
[arch_recipes.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes.rs),
[snapshots.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes/snapshots.rs),
and [tests.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes/tests.rs).

## CI and validation

Use [Choosing coverage](testing.md#choosing-coverage) and the pinned toolchain.
Recipe changes select offline Linux checks, configuration validation/drift,
and PKGBUILD/install-script ShellCheck. Tests cover descriptors, retained pins,
previews, readiness, and safe snapshot cleanup; they do not prove current release
availability or a native Arch build/install. Verify those in a disposable Arch
home/cache. A successful docs check alone does not validate a recipe change.
