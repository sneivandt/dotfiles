# Local package recipes

On Arch Linux, dotfiles can maintain package recipes locally and expose them
to Paru. These recipes repackage official release binaries rather than compile
applications from source.

| Local package | Tracked recipe | Upstream |
|---|---|---|
| `apm-dotfiles` | [packages/apm/](../packages/apm/) | Microsoft APM |
| `github-copilot-app-dotfiles` | [packages/github-copilot-app/](../packages/github-copilot-app/) | GitHub Copilot App |

Dotfiles discovers release versions and prepares build inputs. Paru downloads,
verifies, builds, installs, and upgrades the resulting Arch packages. The agent
package task independently invokes the `apm` executable on PATH.

## Files and ownership

| File or directory | Owner and purpose | Tracked here? |
|---|---|---|
| `packages/apm/PKGBUILD` | Package name, dependencies, release URLs, packaging steps, and packaging revision (`pkgrel`) | Yes |
| `packages/apm/release.json` | Upstream repository, release asset names, and package base | Yes |
| `packages/apm/version.sh` | Discovered upstream version and SHA-256 digests | No |
| `symlinks/config/paru/paru.conf.base` | Shared Paru options, including `CleanAfter` | Yes |
| `~/.config/paru/paru.conf.base` | Home symlink to the tracked base | No |
| `~/.config/paru/paru.conf` | Generated options and local repository paths | No |
| `<CloneDir>/apm-dotfiles/` | Disposable Git snapshot and Paru build output | No |

The snapshot commits are local cache state. They do not commit version pins to
the dotfiles repository, create a remote, or publish anything.

## Normal use

```bash
dotfiles update
paru -Syu
```

During `dotfiles update`:

1. **Local package versions** (`local-package-versions`) discovers current
   release versions and writes changed `version.sh` files.
2. **Paru configuration** (`paru-config`) requires that task to succeed,
   refreshes changed build snapshots, and writes changed Paru configuration.
3. **AUR packages** (`aur-packages`) requires a working Paru and its generated
   configuration. It installs missing configured packages, including
   `apm-dotfiles` from the local repository.
4. **Agent packages** (`apm`) waits for package installation, then runs APM from
   PATH. An unrelated package failure does not prevent it using an available APM.

The default package task installs missing packages. Once `apm-dotfiles` is
installed, use `paru -Syu` to upgrade it after refreshing its available version.
For an explicit first installation, `paru -S apm-dotfiles` also works after
snapshot preparation.

The desktop Arch category also selects `github-copilot-app-dotfiles`. Its
recipe extracts the official Debian package, preserving the app's executable,
desktop entry, and bundled resources. It supports x86_64 and aarch64. The same
version and snapshot tasks discover its `github/app` release assets automatically;
no separate updater or hardcoded version is needed.

Paru does not query GitHub for these release versions. Running Paru without
first running dotfiles uses the last prepared snapshot. Changes to a tracked
recipe also reach Paru when `paru-config` refreshes its snapshot.

## Version discovery

[`release.json`](../packages/apm/release.json) declares:

```json
{
  "package_base": "apm-dotfiles",
  "repository": "microsoft/apm",
  "assets": {
    "aarch64": "apm-linux-arm64.tar.gz",
    "x86_64": "apm-linux-x86_64.tar.gz"
  }
}
```

The update-only version task scans immediate directories under `packages/`, in
sorted order, and reads each `release.json`. This discovery is independent of
`conf/packages.toml`: a recipe can be maintained without being selected for
automatic installation. Directories without a descriptor are skipped.

For APM, the task runs `curl` against:

```text
https://api.github.com/repos/microsoft/apm/releases/latest
```

It reads `tag_name`, rejects drafts and prereleases, strips a leading `v`, and
validates the version before using it as shell data. It finds each named asset
and reads its `sha256:` digest from GitHub's release metadata. It does not
read an AUR version, download a release archive, or execute the PKGBUILD.

The generated file contains `pkgver` and architecture-specific
`sha256sums_<architecture>` arrays. The PKGBUILD sources `./version.sh` and uses
those values in its download URLs and makepkg checksum verification. The
packaging revision, `pkgrel`, stays in the tracked recipe.

All discovered metadata is validated before any version file is written.
Missing assets or digests, invalid descriptors, API rate limits, and network
failures leave existing pins intact and fail the task. There is no stale-data
success fallback. Each changed file is written atomically; identical files
are left alone. This is not a transaction across multiple recipes if a later
filesystem write fails.

A dry run still queries GitHub, but writes neither pins nor snapshots. On a
fresh checkout, snapshot preview can report work before pins exist; applying
snapshot preparation requires the generated pins.

## How Paru sees the recipes

`paru-config` reads the tracked `paru.conf.base` and generates a regular
`~/.config/paru/paru.conf`. It replaces the former config symlink atomically,
without overwriting its tracked source.

For the default cache location, the generated portion is:

```ini
[options]
CloneDir = /home/stuart/.cache/paru/clone

[dotfiles-apm-dotfiles]
Path = /home/stuart/.cache/paru/clone/apm-dotfiles
GenerateSrcinfo
```

Paths are calculated for the current user. The build root follows `AURDEST`
when set, otherwise `$XDG_CACHE_HOME/paru/clone`, falling back to
`~/.cache/paru/clone`. Use the same build-directory environment when updating
dotfiles and running Paru. A later `--clonedir` override can break that alignment.

The task creates one local repository section per package base. `Path` points
to its prepared snapshot, and `GenerateSrcinfo` makes Paru regenerate package
metadata from its PKGBUILD. The package name comes from the recipe, not its
source directory name. Check discovery without upgrading anything with:

```bash
paru -Sl --pkgbuilds
```

Paru's Path repositories use local files immediately, take priority over the
AUR, and skip the normal recipe review step. Review tracked recipe changes
before applying them. See the upstream
[Paru configuration manual](https://github.com/Morganamilo/paru/blob/v2.1.0/man/paru.conf.5).

## Why there is a disposable Git snapshot

`CleanAfter` remains enabled for normal AUR and local package builds. In Paru
2.1.0, post-build cleanup looks for `<CloneDir>/<package_base>` and runs Git
reset/clean there. Building directly from this checkout both misaligns that
path and makes Git cleanup unsuitable for the source repository.

Instead, `paru-config` copies `PKGBUILD` and generated `version.sh` into a
standalone Git repository at exactly that cache path. It commits both files
when their contents change. Paru can then reset tracked inputs to that commit
and remove generated files while retaining the version pins needed next time.
It cannot reset the separate dotfiles checkout.

Snapshots carry an ownership marker inside their own `.git` directory. The
task refuses existing unmanaged directories, linked worktrees, symlinked
snapshot directories, and repositories pointing outside the snapshot. It does
not repurpose an unrelated cached package checkout.

## Adding a recipe

1. Create `packages/<recipe>/PKGBUILD` and `release.json`.
2. Set `package_base` to makepkg's actual package base: the recipe's `pkgbase`,
   or `pkgname` when there is no explicit `pkgbase`. It must be unique among
   local recipes. The directory name may differ.
3. Declare the upstream GitHub repository and release asset name for every
   supported architecture. These names must match published release assets.
4. Have the PKGBUILD source `./version.sh`, declare a tracked `pkgrel`, and use
   the generated version and checksum arrays. Construct release URLs according
   to that upstream's tag convention.
5. Add `{ name = "<package-name>", aur = true }` in the appropriate category of
   `conf/packages.toml` if it should be installed automatically. Otherwise,
   install it explicitly through Paru after preparation.
6. Run the checks below and verify a build on Arch before installing it.

The current snapshot format copies only `PKGBUILD` and `version.sh`. Recipes
needing patches, install scripts, or other local files require extending that
format first. Version discovery currently supports stable GitHub releases
with published asset digests, not arbitrary version commands or VCS recipes.

## Logs and troubleshooting

| Log line | Meaning |
|---|---|
| `Local package versions: write .../version.sh` | Available version/checksum pins changed |
| `Paru configuration: refresh local package snapshot ...` | The recipe or pins changed in Paru's cached build inputs |
| `Paru configuration: write .../paru.conf` | Generated repository paths/options changed |
| AUR package installation or Paru's upgrade output | A package was actually installed or upgraded |

A snapshot refresh alone does not upgrade the installed executable.

If pins are missing, run the version task before applying `paru-config`.
If a cache directory is reported as unmanaged, inspect and preserve it before
moving it out of the way; dotfiles deliberately refuses to overwrite it.
If Paru reports `git reset --hard HEAD: No such file or directory`, check that
its `CloneDir`, the generated repository `Path`, and any `AURDEST` or command-line
overrides agree. The directory basename must match `package_base`.

## CI and validation

Recipe-only changes select Linux Rust checks, configuration validation/drift,
and ShellCheck. They do not request a CLI binary release or Windows package
integration. Rust implementation changes still use the usual cross-platform
CI and release rules. Documentation-only changes retain documentation checks.

The offline checks cover:

- Parsing the actual tracked descriptors and checking required fields,
  safe names, architecture keys, and recipe existence, without generated pins.
- ShellCheck for `packages/*/PKGBUILD`, without traversing downloads, generated
  pins, or build trees.
- Mocked release discovery, metadata failures, preview safety, and repeat runs.
- Snapshot ownership, local commits, refresh behavior, and isolation of Git
  reset from the tracked source checkout.

Run these locally using the pinned Rust toolchain:

```bash
sh .github/workflows/scripts/linux/check.sh fmt clippy test config docs shell ci
```

CI does not query current GitHub releases or download/install APM to validate a
recipe. Its Ubuntu and Windows profile jobs do not run native Arch package
operations. A passing run therefore does not prove release availability,
checksums of newly published downloads, native package builds, or Paru's full cleanup lifecycle. Verify those on Arch in a
disposable build/cache environment. Use build-only Paru operation (`paru -B`)
when testing cleanup, with installation disabled and controlled configuration.
