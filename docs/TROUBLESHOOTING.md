# Troubleshooting

Start by previewing the full run, then inspect the latest log:

```bash
dotfiles install --dry-run --verbose
dotfiles log --verbose
```

Logs are retained per run, so a failure stays readable after later runs. Use
`dotfiles log --list` to enumerate retained runs and `dotfiles log <N>` to read
an earlier one.

Then narrow the command with `--only` and a selector from
[Task reference](TASKS.md).

## The wrapper cannot find or download the binary

Symptoms:

- no compatible release asset
- checksum download failure
- checksum mismatch
- build provenance verification failure
- unsupported architecture

Actions:

1. Confirm GitHub is reachable over HTTPS.
2. Confirm the operating system and architecture have a published asset.
3. Remove only a known incomplete binary download, then rerun the wrapper.
4. Build from the checkout:

```bash
./dotfiles.sh --build --version
```

```powershell
.\dotfiles.ps1 --build --version
```

Do not bypass checksum verification.

Build provenance verification is required by default. An unverifiable
self-update leaves the installed binary unchanged; see
[Security model](SECURITY.md#build-provenance-verification). Do not set
`DOTFILES_SKIP_ATTESTATION=1` unless you explicitly accept bypassing provenance
verification.

## The binary never self-updates

Symptoms:

- an installed binary keeps reporting the same version
- `dotfiles --version` reports a version older than the latest release

Releases are tagged `vYYYY.MM.DD-N`. The self-update check only recognizes that
format, so a binary built before the format changed treats every current
release tag as unparseable and silently declines to update.

Actions:

1. Delete the bootstrap binary at `bin/dotfiles` (`bin\dotfiles.exe` on Windows)
   inside the dotfiles checkout.
2. Rerun a wrapper. It downloads a current release and verifies it normally.

## Cargo build fails

Confirm the repository's required Rust toolchain and native dependencies are
installed, then run:

```bash
cargo build --manifest-path cli/Cargo.toml
```

If only the wrapper build fails, run Cargo directly to see the compiler or
linker error without wrapper output.

## Paru bootstrap reports an incomplete Rust/Cargo prerequisite

The Paru task executes `cargo --version` before cloning or building. On Arch,
install the distribution-managed toolchain and AUR build prerequisites:

```bash
sudo pacman -Syu --needed base-devel git rust sudo
```

If you use `rustup`, install and select a default toolchain before retrying:

```bash
rustup default stable
```

Fresh systems provisioned by `install-arch` use the first approach and verify
Cargo inside the target system before starting dotfiles.

## No profile can be selected

Profile priority is CLI, environment, local Git config, then an interactive
prompt. In a non-interactive environment, set the profile explicitly:

```bash
dotfiles install --profile base
```

To inspect a persisted choice:

```bash
git config --local --get dotfiles.profile
```

An unknown explicit profile is an error; use a name declared in
`conf/profiles.toml`.

## Configuration does not parse

Run:

```bash
dotfiles --root . test --verbose
```

Core required files are `profiles.toml`, `symlinks.toml`, `packages.toml`, and
`manifest.toml`. Common causes include:

- malformed TOML
- a value placed under the wrong section
- a nonexistent symlink source
- a conditional symlink missing manifest coverage
- an invalid package or APM reference

Edit the complete path shown in the diagnostic. Do not assume a similarly named
overlay file caused the error.

## An overlay appears to be ignored

Confirm the path points to the overlay repository root and pass it to both
validation and install:

```bash
dotfiles --root . --overlay C:\path\to\overlay test --verbose
dotfiles --root . --overlay C:\path\to\overlay install --dry-run --verbose
```

Remember:

- supported records append; they do not override main entries
- missing overlay config files are empty
- `manifest.toml` is not loaded from overlays
- `scripts.toml` is loaded only from the overlay
- script paths are relative to the overlay

## A task does not run

A task may not run because:

- its command membership excludes it (`install` excludes update-only tasks)
- it is not applicable to the host
- its configuration list is empty
- `--only` did not match its stable selector or full display label
- `--skip` removed it
- a dependency failed
- current state already matches desired state

Run `dotfiles tasks` to discover stable selectors, then retry with one exact
selector and verbose output:

```bash
dotfiles install --only systemd --dry-run --verbose
```

Use `dotfiles update` for **APM package updates**.

## A symlink cannot be created on Windows

Run:

```powershell
dotfiles install --only "developer mode,symlinks" --dry-run --verbose
```

Confirm Developer Mode is enabled, open a new terminal, and check whether
another file occupies the target. Do not elevate the whole workflow for one
capability.

## A task was skipped because elevation was unavailable

Tasks that need administrator rights are delegated to one short-lived elevated
child run. When that cannot happen, the run continues and reports the reason:

| Skip reason | Meaning |
| --- | --- |
| `elevation declined` | The UAC prompt was dismissed. |
| `elevation unavailable in a non-interactive session` | CI or a session with no console, where no one can answer a prompt. |
| `requires <task>` | A dependency was skipped for one of the reasons above. |

The CLI skips dependent tasks rather than running them without a prerequisite.
Run the specific task from an elevated terminal, then run the normal workflow
again:

```powershell
dotfiles install --only developer-mode
dotfiles install
```

## A profile switch would remove files

Conditional sources may leave the sparse checkout. **Sparse checkout** depends
on **Excluded home files**, which copies linked
content into the home target before applying exclusions.

Always preview profile transitions:

```bash
dotfiles install --profile base --dry-run --verbose
```

If preservation fails, do not force the sparse-checkout change; resolve the
reported source or target problem first.

## Repository update fails

Repository synchronization needs a Git checkout with a usable upstream. Check:

```bash
git status
git remote -v
git branch -vv
```

Resolve authentication, upstream, or conflicting local changes without
discarding user work. `--root` must identify the intended checkout.

If update changes configuration, the CLI reloads it at the dynamic-task
discovery boundary before rebuilding overlay script tasks. Verbose logs show
whether the reload signal was consumed.

## Packages do not install

On Arch, regular packages use pacman and AUR records use paru. Windows packages
use winget.

Check the configured identifier and provider directly, then preview the package
tasks:

```bash
dotfiles install --only packages --dry-run --verbose
```

An AUR failure may originate in **Paru package manager** before **AUR packages**.
Do not mark a provider failure as already installed.

The bootstrap checks the current Arch package database with `pacman -Q paru` and
the installed executable with `/usr/bin/paru --version`. `install-arch` runs
dotfiles through `arch-chroot /mnt`, so both checks inspect the target system,
not the live ISO. A loader failure such as a missing `libalpm.so.*` triggers a
source rebuild. If PATH contains `paru` but the target package is absent, the
task reports it as stale or broken. Later AUR operations call the validated
`/usr/bin/paru` directly.

## APM update is skipped

APM updates require a successful install fingerprint for the current merged
manifest. First converge install state:

```bash
dotfiles install --only apm --verbose
dotfiles update --only apm,apm-update --dry-run --verbose
```

Also confirm active main and overlay fragments are valid and APM is available.

## Optional analyzers are not running

ShellCheck and APM validation are skipped when their executables are absent.
The PowerShell task runs whenever `pwsh` is available; if the PSScriptAnalyzer
module is missing, validation fails with the PowerShell error. Install the
required executable or module, open a new shell if PATH changed, and rerun the
test.

## systemd changes are not visible

Bare unit names use user scope. Check the user manager:

```bash
systemctl --user daemon-reload
systemctl --user status <unit>
```

For an entry configured with `scope = "system"`, inspect the system manager
instead:

```bash
systemctl status <unit>
```

Confirm the unit source was linked and its packages were installed. The task
depends on package, AUR, and symlink convergence.

## WSL settings did not change

Changes to `wsl.conf` usually require a complete distribution shutdown:

```powershell
wsl --shutdown
```

Then start the distribution again. Confirm the task ran inside WSL, not through
the Windows host executable.

## Uninstall did not restore machine defaults

Uninstall materializes symlinks and removes hooks and the wrapper. It does not
remove packages or reverse registry, systemd, shell, WSL, editor, APM, or
arbitrary script changes. See
[Uninstall tasks](TASKS.md#uninstall-tasks).
