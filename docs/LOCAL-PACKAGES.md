# Local package recipes

On Arch Linux, dotfiles can maintain package recipes locally and expose them
to Paru. These recipes repackage official release binaries rather than compile
applications from source.

| Local package | Tracked recipe | Upstream |
|---|---|---|
| `apm-dotfiles` | [packages/apm/](../packages/apm/) | Microsoft APM |
| `github-copilot-app-dotfiles` | [packages/github-copilot-app/](../packages/github-copilot-app/) | GitHub Copilot App |
| `powershell-dotfiles` | [packages/powershell/](../packages/powershell/) | Microsoft PowerShell |
| `github-copilot-cli-dotfiles` | [packages/github-copilot-cli/](../packages/github-copilot-cli/) | GitHub Copilot CLI |
| `openai-codex-dotfiles` | [packages/openai-codex/](../packages/openai-codex/) | OpenAI Codex CLI |

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
   release versions independently and writes each successful recipe’s changed
   `version.sh` file. Failed refreshes retain that recipe’s existing pins and
   remain failures in the run summary.
2. **Paru configuration** (`paru-config`) waits for discovery to finish,
   refreshes usable build snapshots, and writes Paru configuration containing
   only available recipes. Existing pins can be used after a failed refresh.
3. **AUR packages** (`aur-packages`) requires a working Paru and waits for
   configuration preparation. It installs missing configured packages whose
   inputs are ready, including `apm-dotfiles` from the local repository. Missing
   local packages without prepared inputs are counted as failures and excluded
   from installation; unrelated AUR packages continue. Unavailable local package
   names are never submitted as direct AUR install targets.
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

The Arch category selects `powershell-dotfiles` instead of `powershell-bin`.
It packages Microsoft's x86_64 and aarch64 archives with the bundled .NET runtime
and installs `pwsh` under `/opt/microsoft/powershell/<major>`, linked from
`/usr/bin/pwsh`. Its `package.install` script registers PowerShell in
`/etc/shells` on install/upgrade and removes that entry on uninstall.
The first installation conflicts with the existing `powershell-bin` package;
review that replacement when Paru installs it.

The Arch category also selects `github-copilot-cli-dotfiles` and
`openai-codex-dotfiles` instead of their AUR binary packages. Both use official
GitHub release archives, install shell completions and upstream licenses, and
declare conflicts with their previous packages. Codex preserves the complete
upstream bundle, including its code-mode helper and voice runtime. The Copilot
recipe installs Bash, Zsh, and Fish completions; Codex also installs Elvish and
PowerShell completions. Optional features retain their declared dependencies.

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
automatic installation. Directories without a recipe or descriptor are skipped;
a PKGBUILD without its descriptor is an error.

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

Asset names may contain `{version}`, which is replaced with the validated
release version without the leading `v`. The PowerShell descriptor uses this
for archives such as `powershell-7.6.9-linux-x64.tar.gz`.

For other tag conventions, an optional `tag_prefix` declares the exact prefix
to strip before version validation. Codex uses `"tag_prefix": "rust-v"`.
A configured prefix must match; omitting it retains optional leading-`v`
handling. Tracked `LICENSE` and `NOTICE` files are packaging inputs, not
automatically refreshed release metadata; review them when upstream terms change.

Descriptors are validated before network requests or writes, including unique
package bases and package names. Invalid ownership metadata fails the task and
prevents AUR installation because the installer cannot safely identify local
packages.

Release metadata is validated separately for each recipe, including every
configured architecture. Missing assets or digests, API rate limits, and network
failures preserve that recipe’s pins and count as failures; successful recipes
still publish their updates. File write failures also leave other recipes free
to proceed. Each changed file is written atomically; identical files are left
alone. Cancellation propagates immediately.

Refresh success and package readiness are separate. A failed refresh does not
prevent preparing a snapshot from existing pins, and the refresh failure remains
visible. Missing pins or snapshot ownership failures make only that recipe
unavailable. The installer checks current committed snapshot inputs and the
published repository section before requesting a missing local package. Already
installed packages need no reinstall. Errors reading or parsing ownership
metadata fail closed rather than treating local packages as ordinary AUR names.

A dry run still queries GitHub, but writes neither pins nor snapshots. On a
fresh checkout, snapshot preview can report work before pins exist; applying
snapshot preparation requires the generated pins.

## How Paru sees the recipes

`paru-config` reads the tracked `paru.conf.base` and generates a regular
configuration file. Its destination follows `PARU_CONF`, then
`$XDG_CONFIG_HOME/paru/paru.conf`, then `~/.config/paru/paru.conf`.
Overrides must be absolute and nonempty. Generation and package readiness use
the same destination; use the same environment when running dotfiles and Paru.
When the XDG destination does not exist yet, dotfiles creates it so Paru uses it
instead of falling back to a home or system configuration. The task replaces
the former config symlink atomically, without overwriting its tracked source.

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

Instead, `paru-config` copies `PKGBUILD`, generated `version.sh`, and an optional
`package.install` script and `LICENSE`/`NOTICE` files into a
standalone Git repository at exactly that cache path. It commits both files
when their contents change. Paru can then reset tracked inputs to that commit
and remove generated files while retaining the version pins needed next time.
It cannot reset the separate dotfiles checkout.

Snapshots carry an ownership marker inside their own `.git` directory. The
task refuses existing unmanaged directories, linked worktrees, symlinked
snapshot directories, and repositories pointing outside the snapshot. It does
not repurpose an unrelated cached package checkout.

Before publishing repository entries, apply runs `makepkg --printsrcinfo` on
temporary copies of each recipe and its pins. Package base, package names, and
architectures must match `release.json`. This check also runs when only the
descriptor changed. A mismatch or makepkg failure preserves the cached inputs,
omits that recipe from the generated configuration, and counts as a failure;
independent recipes can still proceed. Metadata generation executes the
PKGBUILD, so only reviewed recipes should be used. Temporary copies protect
build inputs but do not sandbox that code. Dry runs do not execute PKGBUILDs or
create temporary build inputs; metadata validation is deferred until apply.

## Adding a recipe

1. Create `packages/<recipe>/PKGBUILD` and `release.json`.
2. Set `package_base` to makepkg's actual package base: the recipe's `pkgbase`,
   or `pkgname` when there is no explicit `pkgbase`. It must be unique among
   local recipes. The directory name may differ. The owned package name defaults
   to `package_base`, so ordinary single-package recipes need no extra field.
   For split packages or a differing package name, add a `packages` array listing
   every actual package name. Names must be unique across descriptors. This
   ownership information lets the installer refuse AUR fallback without executing
   the PKGBUILD.
3. Declare the upstream GitHub repository and release asset name for every
   supported architecture. These names must match published release assets.
4. Have the PKGBUILD source `./version.sh`, declare a tracked `pkgrel`, and use
   the generated version and checksum arrays. Construct release URLs according
   to that upstream's tag convention.
5. Add `{ name = "<package-name>", aur = true }` in the appropriate category of
   `conf/packages.toml` if it should be installed automatically. Otherwise,
   install it explicitly through Paru after preparation.
6. Run the checks below and verify a build on Arch before installing it.

The snapshot format copies `PKGBUILD`, `version.sh`, and optional
`package.install`, `LICENSE`, and `NOTICE` files.
Use `install=package.install` for package lifecycle hooks. Recipes needing
patches or other local files require extending that format first.
Version discovery currently supports stable GitHub releases
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
- ShellCheck for `packages/*/PKGBUILD` and `packages/*/*.install`, without traversing downloads, generated
  pins, or build trees.
- Mocked release discovery, metadata failures, preview safety, and repeat runs.
- Independent refreshes and AUR installs after an upstream failure, with and
  without retained pins, under sequential and parallel scheduling.
- Unavailable local package filtering, published-config readiness, and isolation
  of snapshot ownership failures.
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
