# Windows guide

Run dotfiles as your normal Windows user. The PowerShell wrapper obtains the
CLI; the CLI can delegate specific privileged work without elevating the whole
configuration run. Windows and WSL are separate installations with separate
home directories and capabilities.

For shared command semantics, use [Usage](USAGE.md); for file formats, use
[Configuration](CONFIGURATION.md).

## Requirements

| Requirement | Needed for |
|---|---|
| x86-64 Windows | The published `dotfiles-windows-x86_64.exe` release asset |
| PowerShell | Running `dotfiles.ps1`; the installed launcher prefers `pwsh`, then Windows PowerShell |
| Git | Repository synchronization, local selection persistence, and hooks |
| winget | Installing configured Windows packages |
| `gh` | Build-provenance verification; initial bootstrap warns if it is absent |
| Rust/Cargo and native build prerequisites | Only a source build with wrapper `--build`; see [Contributing](CONTRIBUTING.md) |
| `pwsh` and PSScriptAnalyzer | PowerShell repository validation, separate from bootstrap |

The wrapper rejects unsupported Windows release architectures. A source build
requires an appropriate Rust target/toolchain; absence of a published asset is
not evidence that another architecture has native runtime coverage.

Keep the checkout at a stable path. Links and the installed launcher refer
back to it. Review `conf/` and back up existing home configuration first:
installing managed links can replace regular files without backup.

## Bootstrap

Open PowerShell in the intended checkout. These commands first discover tasks,
then preview configuration:

```powershell
# Downloads/verifies the CLI if it is missing; does not run install tasks.
.\dotfiles.ps1 tasks --root . --profile desktop

# Use the binary directly once available.
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --dry-run --verbose
```

Use `base` instead of `desktop` for the shell-focused role. The explicit profile
does not change a previously saved default. See [Profiles](PROFILES.md).

**Bootstrap is not covered by CLI dry-run.** Even wrapper help or discovery can
download files into `bin\`. The CLI preview itself writes logs/lock metadata and
can check/cache the latest release. To suppress that release check for a preview
without leaving the environment changed:

```powershell
$previousSkipSelfUpdate = $env:DOTFILES_SKIP_SELF_UPDATE
try {
    $env:DOTFILES_SKIP_SELF_UPDATE = '1'
    .\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --dry-run --verbose
} finally {
    $env:DOTFILES_SKIP_SELF_UPDATE = $previousSkipSelfUpdate
}
```

Only after reviewing planned changes and backing up affected files, **apply**
the current checkout:

```powershell
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update
```

This can update the CLI binary before applying tasks. `--no-repo-update` keeps
repository content fixed; it is not a general network or self-update switch.
See [Dry-run scope](USAGE.md#dry-run-scope).

For a source build that only discovers tasks:

```powershell
.\dotfiles.ps1 --build tasks --root . --profile desktop
```

The wrapper executes Cargo's actual artifact, including custom target/output
locations. It does not need to put a source-built binary in `bin\`.

### Download and update failures

Bootstrap verifies SHA-256 and, when `gh` is present, provenance before
installing the download. Do not turn off verification to work around a failed
download. See [Troubleshooting](TROUBLESHOOTING.md#the-wrapper-cannot-find-or-download-the-binary).

Self-update renames the running executable to a backup, puts the verified
download in place, and smoke-tests `--version`. A failed smoke test attempts
to restore the backup; a rollback failure is reported explicitly. Windows
sharing violations can prevent replacement or rollback. The updated process
runs to completion in the same console; a normal self-update is not a
background installation.

## Developer Mode and symlinks

In the full graph, **Home symlinks** requires **Windows Developer Mode**.
Developer Mode lets ordinary users create file symlinks. Directory symlinks can
fall back to junctions, but that fallback does not remove the graph dependency
or make file links unprivileged.

To inspect both operations rather than assuming Developer Mode is ready:

```powershell
.\bin\dotfiles.exe tasks --root . --profile desktop --graph install --only symlinks --with-deps
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --only symlinks --with-deps --dry-run --verbose
```

`--only symlinks` without `--with-deps` deliberately omits the Developer Mode
task. Use it when you know the prerequisite is already satisfied, not as a way
to fix an unmet prerequisite.

There are two distinct symlink problems:

- **Home target cannot be linked:** inspect Developer Mode/elevation and the
  target named in the error. Existing regular files or empty directories may
  be replaced without backup; nonempty directories fail. Back up and relocate
  unrelated content rather than deleting it blindly.
- **Source is a Git symlink placeholder:** Git recorded a symlink, but the
  checkout contains an ordinary file with its target text. Enabling Developer
  Mode afterward does not convert that file. Preserve local edits, enable Git
  symlink support, and restore only affected source entries or create a fresh
  symlink-capable checkout. The CLI intentionally rejects placeholders instead
  of deploying their text as configuration.

A read-only inspection from the checkout:

```powershell
git config --show-origin --get core.symlinks
git ls-files --stage -- symlinks | Select-String '^120000 '
```

No explicit `core.symlinks` value does not by itself diagnose the problem;
compare the reported source's on-disk type with its Git mode.

## Elevation

The broker plans elevation only for applicable tasks whose current state needs
a privileged change:

| Task | Reason for Administrator rights |
|---|---|
| Windows Developer Mode | Machine policy value is not enabled |
| Home symlinks | File links need creation/correction while unprivileged symlinks are unavailable |

Other Windows catalog tasks do not request brokered elevation. **That does not
mean every external installer is user-scope:** winget can launch an installer
with its own UAC prompt.

The broker names the tasks and requests one short-lived elevated child,
restricted to their exact selectors with `--only` and `--no-parallel`.
The child has a separate console and run log; dependency expansion and the
parent's broader task filters are not forwarded. The normal parent stays
unelevated. Its history links to the child's run ID.

- **Consent declined or no interactive console:** affected tasks are skipped,
  blocking dependents are blocked, independent tasks continue.
- **`--fail-on-skip` or CI:** unavailable required elevation makes the command
  fail rather than succeed with unmet work.
- **Elevated child failed:** the command fails and its dependents are blocked.
- **Everything already current:** no broker prompt is needed.

Dry-run does not request elevation to apply planned changes. After resolving a
policy/consent issue, rerun from the ordinary terminal. If you intentionally
use an elevated terminal for Developer Mode only, scope the actual mutation:

```powershell
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --only developer-mode
```

Then return to the ordinary terminal for the rest. Do not routinely run the
whole workflow as another Administrator account; user-scope targets belong to
the current user's environment.

## Packages

Windows package declarations use exact winget IDs. The task queries installed
state and installs only missing configured packages; selecting `update` does
not request `winget upgrade --all`.

```powershell
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --only packages --dry-run --verbose
```

If `winget list` fails or its `Id` column cannot be parsed, the task fails
before treating anything as missing, including during dry-run. Check the
provider/version and retained output; do not interpret a failed inventory as
an empty machine.

Apply prints each package ID and:

1. Tries the installer with `--scope user`.
2. Retries without a scope only for “no applicable installer.”
3. Reports administrator-required, cancelled, or policy-blocked packages as
   unmet/skipped and continues with other packages.

Winget's own interaction is disabled and source/package agreements are
accepted by these commands; an installer's UAC dialog can still appear.
Strict skip policy fails if applicable work remains. Review package sources
and installation scope before applying.

If a package truly requires Administrator rights, an intentionally elevated,
package-only rerun can complete it. That still covers **all** active configured
packages; `--only` does not accept a package ID. A policy block requires a
policy decision, not repeated UAC attempts.

## Registry settings

The registry task applies declared current-user values from
`conf/registry.toml` while preserving undeclared values. Registry tables are
named settings groups, not role categories. Review them even for the `base`
profile.

```powershell
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --only registry --dry-run --verbose
```

Current settings include console, regional formatting, Explorer, taskbar,
search, Start, desktop icons, and window behavior. Opaque taskbar/Start pin
formats are intentionally not managed. The task is not a registry backup.
Some values are read only when the affected application or user session next
starts; save work before deliberately restarting either.

## PATH and wrapper

The launcher task writes `%USERPROFILE%\.local\bin\dotfiles.cmd`. It calls the
repository's `dotfiles.ps1` with `pwsh` if available, otherwise
`powershell.exe`. The PATH task adds that directory to the persistent user PATH,
preserving expandable registry tokens and the registry value type.

Open a new terminal after first install. If `dotfiles` still cannot be found:

```powershell
Get-Command dotfiles -All
Test-Path "$env:USERPROFILE\.local\bin\dotfiles.cmd"
```

Continue using the explicit checkout wrapper/binary until PATH is correct.
Multiple launchers can select a different checkout; use `--root` to choose
configuration explicitly, and that checkout's wrapper when its binary matters.

## PowerShell configuration

Profile content is delivered from `symlinks/` by the active Windows category.
Edit tracked sources rather than installed links. New sessions load profile
changes; completion registration is managed by its own `completions` task.

Bootstrap compatibility with Windows PowerShell does not imply analyzer
compatibility. The repository check requires **`pwsh`**, and once present it
requires the PSScriptAnalyzer module. Missing `pwsh` is a skip; missing the
module is a failed check. See [CLI validation](TESTING.md#cli-validation).

## WSL

Run the **Linux** wrapper/binary inside the distribution for its Linux home
configuration. Running the Windows executable on the host does not manage
`/etc/wsl.conf`, even if the current path happens to expose Linux files.

The Linux **System files** task merges WSL-selected settings: systemd enabled
and Windows PATH injection disabled, preserving unrelated INI settings. It
can need sudo inside the distribution and is not selected on native Linux or
the Windows host.

Save work before applying the required restart. From Windows, this shuts down
**all running WSL distributions** and their processes:

```powershell
wsl --shutdown
```

Then reopen the distribution. Until that restart, the systemd task can remain
inapplicable because editing the configuration did not start the manager.

## Uninstall

Preview with the original profile/overlay while the sources remain available:

```powershell
.\bin\dotfiles.exe uninstall --root . --profile desktop --dry-run --verbose
```

After review, removing `--dry-run` materializes configured home links and
removes hooks and the launcher still matching their managed state. Modified
hooks/launchers and unrelated hook names are preserved. Active overlay scripts
can also run their removal actions. This is **not** restoration of original Windows
settings: winget packages, Developer Mode, registry values, PATH, editor
extensions, and APM state remain. See [Uninstall tasks](TASKS.md#uninstall-tasks).
