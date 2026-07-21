# Troubleshooting

Start with a dry run and verbose output:

```bash
dotfiles install --dry-run --verbose
dotfiles log --verbose
```

Then narrow the command with `--only` using a display name from
[Task reference](TASKS.md).

## The wrapper cannot find or download the binary

Symptoms:

- no compatible release asset
- checksum download failure
- checksum mismatch
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

## Cargo build fails

Confirm the repository's required Rust toolchain and native dependencies are
installed, then run:

```bash
cargo build --manifest-path cli/Cargo.toml
```

If only the wrapper build fails, running Cargo directly usually exposes the
underlying compiler or linker error without wrapper output.

## No profile can be selected

Profile priority is CLI, environment, local Git config, then an interactive
prompt. In a non-interactive environment, provide one explicitly:

```bash
dotfiles install --profile base
```

To inspect a persisted choice:

```bash
git config --local --get dotfiles.profile
```

An unknown explicit profile is an error; use a name declared in
`conf\profiles.toml`.

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

Use the complete path from the reported diagnostic rather than editing a
similarly named overlay file.

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

A task can be absent from execution because:

- its phase is excluded (`install` excludes Update)
- it is not applicable to the host
- its configuration list is empty
- `--only` did not match its display name
- `--skip` removed it
- a dependency failed
- current state already matches desired state

Retry with the full display name or a canonical selector and verbose output.
Selectors are normalized whole names or tokens, not arbitrary substrings:

```bash
dotfiles install --only "Configure systemd units" --dry-run --verbose
```

Use `dotfiles update` for **Update APM packages**.

## A symlink cannot be created on Windows

Run:

```powershell
dotfiles install --only "developer mode,symlinks" --dry-run --verbose
```

Confirm Developer Mode is enabled, open a new terminal, and check whether an
unrelated file already occupies the target. Avoid running the whole workflow
elevated when only a specific capability requires it.

## A profile switch would remove files

Conditional sources may leave the sparse checkout. The Sync phase first runs
**Materialize excluded symlinks**, copying linked content into the home target
before applying exclusions.

Always preview profile transitions:

```bash
dotfiles install --profile base --dry-run --verbose
```

If preservation fails, do not force the sparse-checkout change; resolve the
reported source or target problem first.

## Repository update fails

Repository synchronization requires a suitable Git checkout and upstream. Check:

```bash
git status
git remote -v
git branch -vv
```

Resolve authentication, upstream, or conflicting local changes without
discarding user work. `--root` must identify the intended checkout.

If update changes configuration, the CLI reloads it before Provision tasks.
Verbose logs show whether the reload signal was consumed.

## Packages do not install

On Arch, regular packages use pacman and AUR records use paru. On Windows,
packages use winget.

Check the configured identifier and provider directly, then preview the package
tasks:

```bash
dotfiles install --only packages --dry-run --verbose
```

An AUR failure may originate in **Install paru** before **Install AUR packages**.
Do not mark a provider failure as already installed.

## APM update is skipped

APM updates require a successful install fingerprint for the current merged
manifest. First converge install state:

```bash
dotfiles install --only APM --verbose
dotfiles update --only APM --dry-run --verbose
```

Also confirm active main and overlay fragments are valid and APM is available.

## Optional analyzers are not running

ShellCheck and APM validation are skipped when their executables are absent.
The PowerShell task runs whenever `pwsh` is available; if the PSScriptAnalyzer
module is missing, validation fails with the PowerShell error. Install the
required executable or module, open a new shell if PATH changed, and rerun the
test.

## systemd changes are not visible

The task manages user units. Check the user manager:

```bash
systemctl --user daemon-reload
systemctl --user status <unit>
```

Confirm the unit source was linked and its packages were installed. The task
depends on package, AUR, and symlink convergence.

## WSL settings did not change

`wsl.conf` changes generally require the distribution to stop completely:

```powershell
wsl --shutdown
```

Then start the distribution again. Confirm the task ran inside WSL, not through
the Windows host executable.

## Uninstall did not restore machine defaults

This is intentional. Uninstall materializes symlinks and removes hooks and the
wrapper. It does not remove packages or reverse registry, systemd, shell, WSL,
editor, APM, or arbitrary script changes. See
[Uninstall tasks](TASKS.md#uninstall-tasks).
