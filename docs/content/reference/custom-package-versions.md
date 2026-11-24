---
title: Custom package versions
description: Refresh upstream release pins for managed Arch package recipes and keep release.json separate from generated PKGBUILD snapshots.
---

# Custom package versions

Custom Arch recipes need an upstream release version and asset checksums
before they can be prepared for Paru. This task refreshes those pins from
release descriptors. It is the discovery step, not an application installer
or a request to upgrade software already present on the machine.

| Task detail | Value |
|---|---|
| Selector | `custom-package-versions` |
| Commands | `update` |
| Availability | Arch/AUR support; work depends on managed recipes |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md) |
| Implementation | [packages/arch_recipes.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/packages/arch_recipes.rs) |

## What it does

Finds stable upstream releases and refreshes the version pins for dotfiles-managed Arch
recipes. On platforms with Arch/AUR support, it discovers every managed recipe
under the main checkout's `packages/` directory, not just packages selected by
the current profile, because package declarations control installation rather than
discovery or preparation.

A failed refresh is still a task failure. Existing pins may still produce a usable
snapshot, but they do not make the refresh itself successful. [Custom package
repository](custom-package-repository.md) prepares those snapshots next, and [AUR
packages](aur-packages.md) installs any selected packages.

## Configuration

Each managed recipe has a `PKGBUILD` and release descriptor under
[packages/](https://github.com/sneivandt/dotfiles/tree/main/packages/). The descriptor
defines how to discover a stable upstream version, and any
[existing recipe](https://github.com/sneivandt/dotfiles/tree/main/packages/)
serves as a format reference. Adding a recipe includes it in discovery and preparation;
add an `aur = true` entry in `conf/packages.toml` separately to request installation.

The [PKGBUILD manual](https://man.archlinux.org/man/PKGBUILD.5.en) defines
Arch's package version, release, source, and checksum fields, and Dotfiles layers
its own release descriptor and generated pin file on top.
Edit the tracked descriptor or recipe to change discovery or packaging;
avoid maintaining generated pins by hand.

Preview can query GitHub release metadata, but it never writes pins or release
caches or executes recipes. Refreshing pins and preparing a snapshot still leave
installation to [AUR packages](aur-packages.md). Keeping those three steps separate
helps you tell whether a failure came from release discovery, recipe preparation,
or package installation.

## Preview and apply

Preview release discovery with your configured launcher:

```sh
dotfiles update --only custom-package-versions --dry-run
```

Each `update <recipe>` line identifies changed `version.sh` pins by recipe
directory name, such as `update openai-codex`. Preview and apply use the
same action wording, and `--verbose` adds the pin file paths. A release-query
failure is not proof that existing pins are current, so resolve any reported metadata,
authentication, or network problem before retrying. Once the diff looks right,
[apply the reviewed change](task-reference.md#preview-and-apply). Refreshing pins
alone does not prepare snapshots or install applications.

## Removal

This task has no remove action. Removing a recipe does not uninstall packages previously
built from it; handle package removal separately from changes to release descriptors and
generated pins.
