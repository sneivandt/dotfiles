---
title: Troubleshooting
description: Diagnose failed or skipped tasks, bootstrap and package problems, configuration errors, and platform-specific symptoms.
---

# Troubleshooting

**Read the failed run before retrying a full update.** A retry can update the
CLI, sync repositories, or repeat external work. Earlier successful changes
are not rolled back when a later task fails.

1. Use the exact `dotfiles log --id … -v` command printed after the failure.
2. Check its profile, platform, overlay, and requested tasks.
3. Find the first failed or unmet task, not just dependents marked blocked.
4. Fix that cause, preview a focused rerun, then apply it when ready.

Examples assume you are in the intended checkout with `dotfiles` on PATH.
If the launcher is uncertain, substitute an **existing** `./bin/dotfiles` or
`.\bin\dotfiles.exe`. A wrapper may download/build even before showing help.

## Find the symptom

| Problem | Go to |
|---|---|
| No usable CLI | [Bootstrap](#the-wrapper-cannot-find-or-download-the-binary), [source build](#cargo-build-fails) |
| Old version or a stalled upgrade | [Self-update](#the-binary-never-self-updates), [hidden credential popup](#update-waits-with-a-hidden-wsl-popup) |
| Selection or parsing error | [Profile](#no-profile-can-be-selected), [configuration](#configuration-does-not-parse), [overlay](#an-overlay-appears-to-be-ignored) |
| Missing/skipped work | [Task selection](#a-task-does-not-run), [elevation](#a-task-was-skipped-because-elevation-was-unavailable) |
| Concurrent or failed repository run | [Run lock](#another-run-holds-the-repository-lock), [Git synchronization](#repository-update-fails) |
| Packages or tooling | [Packages](#packages-do-not-install), [Paru](#paru-bootstrap-reports-an-incomplete-rustcargo-prerequisite), [agent packages](#agent-package-update-does-not-run-or-fails), [analyzers](#optional-analyzers-are-not-running) |
| Windows links | [Symlinks](#a-symlink-cannot-be-created-on-windows) |
| Settings not active | [systemd](#systemd-changes-are-not-visible), [keyring](#gnome-keyring-does-not-unlock-at-login), [WSL](#wsl-settings-did-not-change) |
| Removal left settings behind | [Remove is not rollback](#remove-did-not-restore-machine-defaults) |

## Reading logs and partial results

```bash
dotfiles log --list
dotfiles log --task symlinks --verbose
```

The second command examines the **newest** run. For an older failure, add its
actual `--id` from history; IDs stay stable when new runs are added.

Read captured command diagnostics, not just a short task summary or an
“update available” notice. Failed-command output and nonempty stderr normally
appear without verbose; `--verbose` adds checks and other diagnostics.
Successful stdout is usually not retained, so `--raw` cannot recover it.

Restarted/elevated children have their own linked logs. If the parent only
reports a restart or delegation, inspect the child. `unfinished` means no finish
record exists; that can also describe a process still running.

If logging failed, check free space and permissions at the
[log location](commands-and-usage.md#log-locations). Sanitize private paths, configuration,
and external tool output before sharing logs.

## The wrapper cannot find or download the binary

| Error | Check |
|---|---|
| Cannot resolve a release repository | The checkout's GitHub `origin`, or an explicit `DOTFILES_RELEASE_REPO=owner/repo` |
| Latest release/download unavailable | DNS, proxy, HTTPS access, rate limits, and whether that publisher has releases |
| Unsupported architecture | Published assets support Linux x86-64/AArch64 and Windows x86-64 |
| Missing checksum or checksum mismatch | The matching release asset/checksum file; do not execute the failed download |
| Provenance verification failed | `gh`, authentication, network access, and the verification error |
| Binary cannot execute | OS/architecture, Linux executable permission, and local file-access policy |

Forks do not silently fall back to this project's binaries. A fork without
releases needs a deliberate publisher override or a source build; the
[setup guide](getting-started.md#1-choose-a-permanent-checkout) shows both choices.

Bootstrap warns and continues if `gh` is absent. If `gh` is present,
verification failure blocks it; self-update also refuses unverifiable updates.
Do not bypass checksums or set `DOTFILES_SKIP_ATTESTATION=1` as a routine fix.
See [Build provenance verification](security-and-trust.md#build-provenance-verification).

Wrappers clean failed downloads. Do not delete the checkout or all of `bin/`.
If one stale binary must be moved aside, first confirm no run is using it.

## The binary never self-updates

Check these before replacing anything:

- Only update/remove trigger CLI self-update; help, version, list, log,
  and check do not.
- The binary must be an eligible release build, have an embedded publisher,
  and run from the selected checkout's `bin/`.
- `DOTFILES_SKIP_SELF_UPDATE=1` disables the check.
- Dry-run reports an available update but does not install it.
- A fresh cache can delay another lookup for up to ten minutes. Linux cache
  entries also need to match the current boot.
- A failed release lookup leaves the existing binary usable; a failed download
  or verification is a separate error.

Changing `origin` or `DOTFILES_RELEASE_REPO` does not change an existing
binary's embedded self-update publisher.

For authentication, release discovery prefers nonempty `GH_TOKEN`, then
`GITHUB_TOKEN`, then an unattended saved GitHub CLI login. An expired explicit
token can override a working login; correct or unset it. Denied credentials
are not silently retried anonymously.

Anonymous requests share a lower GitHub quota with other users of the same
public IP. Respect a reported retry deadline instead of repeatedly retrying.
Normal saved authentication can be set up with `gh auth login --hostname github.com`;
Linux keyring-only credentials need the special handling below.

If a very old binary cannot recognize current `vYYYY.MM.DD-N` releases, preserve
that one executable and bootstrap again with its checkout's wrapper. Never
bypass download verification to force an upgrade.

### Update waits with a hidden WSL popup

“Updating to …” includes `gh attestation verify`, not just downloading. Older
versions can block on a hidden Linux keyring credential dialog.

Current automatic release checks and verification disable terminal prompts
and Linux desktop-keyring access in their own `gh` processes. Ordinary `gh`
commands and system keyring settings are unchanged. Environment tokens and
non-keyring saved credentials still work.

If verification needs authentication, provide `GH_TOKEN` or `GITHUB_TOKEN`
through your normal credential handling; do not paste tokens into logs.
For an older binary, `DOTFILES_SKIP_SELF_UPDATE=1` avoids its updater without
accepting an unverified download. **It does not disable the rest of update.**
A source build is another route to the fix.

## Cargo build fails

Use the [build prerequisites](contributing.md) and read the first compiler or
linker error, not just the wrapper's final status.

To build and print a version without applying configuration:

```bash
./dotfiles.sh --build --version
```

On Windows:

```powershell
.\dotfiles.ps1 --build --version
```

Builds write artifacts and may download dependencies. The wrapper executes
Cargo's actual `dev-opt` artifact, including custom target/output paths.
Do not copy an arbitrary old executable into `bin/` to hide a build failure.

## No profile can be selected

```bash
dotfiles list --profile base
git config --local --get dotfiles.profile
```

Use exactly `base` or `desktop`. Priority is CLI, nonempty `DOTFILES_PROFILE`,
saved Git setting, then interactive prompt. A bad environment value does not
fall back to the saved setting; surrounding whitespace is not trimmed.

`list` never prompts for a profile. Update/remove/check also need an existing
selection in unattended or JSON Lines mode, with non-terminal stdin, or with
any present `CI` variable. An explicit profile overrides the saved choice
without updating it. See [Profile selection](profiles-and-categories.md#resolution-priority).

## Configuration does not parse

Start with the filename and complete diagnostic. Common causes are a missing
required file, malformed TOML, wrong value type, unsupported key/category,
conflicting targets, unsafe path, or missing source.

**Configuration loads before task filtering.** `--only symlinks` cannot bypass
malformed package configuration. Inactive categories are not a safe place to
leave invalid definitions.

Use [Configuration validation](configuration-reference.md#validation) to interpret the
finding and [CLI validation](testing.md#cli-validation) to recheck it.
`check` does not repair files, and `config-diagnostics` fails on warning-severity
diagnostics too.

## An overlay appears to be ignored

For a sibling overlay called `dotfiles-private`, inspect it from the main
checkout:

```bash
git config --local --get dotfiles.overlay
dotfiles list --overlay ../dotfiles-private
```

Then check:

1. The selected path is the overlay **root**, not its `conf/` directory.
2. A higher-priority flag or environment value is not overriding the saved path.
3. Its sections match the active categories.
4. Overrides match the resource's identity, not just a table name or position.
   Inactive overlay entries do not replace active public entries.
5. Script declarations live in overlay `conf/scripts.toml`, with script paths
   relative to that overlay root.

Missing optional files add nothing, so a wrong path can look like an empty
overlay. See [Private overlays](private-overlays.md#selection-and-persistence)
for selection, persistence, and linked-worktree requirements.

## A task does not run

Inspect selection before executing more work:

```bash
dotfiles list
dotfiles list --graph update --only systemd
```

| What you see | What to check |
|---|---|
| Unknown selector | Use an exact selector/full label from list, not a substring or package ID |
| Missing from the command | Look under the correct update/remove/check group; try `list --all` |
| `filtered` or `skipped` in the graph | Check both `--only` and `--skip` |
| Omitted prerequisite warning | Decide whether to include `--with-deps` |
| No normal result row | The task may be current or inapplicable; inspect verbose output/logs |
| `requires …` or blocked | Fix the named prerequisite first |
| Missing capability/tool | Provide that prerequisite deliberately |
| No active entries | Check the profile/category selection |

For a focused preview:

```bash
dotfiles update --only systemd --dry-run --verbose
```

Add `--with-deps` only after reviewing its graph: it can bring in package
installation, AUR setup, links, and permissions. It is not a narrower retry.

## Another run holds the repository lock

Update, remove, and check share a lock, including previews. Linked worktrees
use the same common Git directory. The error identifies the lock and, when
available, the owner PID, command, and start time.

Wait for the owner or inspect it. `list` and `log` remain usable.
**Do not delete the lock file to force concurrent runs.** The process holds
the lock; leftover owner text alone does not keep it locked.

For a stuck run, follow the [interrupt procedure](commands-and-usage.md#interrupting-a-run).
After force quit, check for surviving child installers before retrying.

## Repository update fails

Inspect the affected main or overlay checkout:

```bash
git status --short
git branch -vv
git remote -v
```

Check remote URLs for credentials before sharing the output.

| Cause | Recovery |
|---|---|
| Tracked local changes | Preserve and resolve them; synchronization skips a dirty checkout |
| Detached HEAD | Repository update is inapplicable; choose the intended branch if appropriate |
| Missing upstream | Configure the intended upstream or deliberately keep current content |
| Local/diverged commits | Reconcile the branch yourself; dotfiles does not reset or rebase |
| Authentication/network failure | Inspect the failed Git command and credentials |
| Fast-forward merge failure | Protect local content, including untracked files that collide |

Untracked files do not fail the readiness check, but can still prevent a merge.
Main and overlay updates are not one transaction.

Use `--no-repo-update` when you deliberately want to apply the current checkout.
It does not discard changes or suppress other network activity.
“Repository synced · restarting to load configuration” is normal: the child
reloads configuration while the parent retains the lock.

## A task was skipped because elevation was unavailable

| Reason | Meaning |
|---|---|
| `elevation declined` | Windows UAC consent was dismissed |
| `elevation unavailable in a non-interactive session` | No supported prompt path |
| `sudo credentials unavailable` | Linux sudo was missing, denied, or could not be primed |
| `sudo credentials unavailable in a non-interactive session` | No usable cached/passwordless authorization |
| `requires <task>` | A blocking prerequisite was left unmet |
| `ran in elevated session` | Work was delegated; inspect the child log |

Windows delegates only selected tasks. Linux primes sudo credentials and uses
sudo for privileged commands; neither requires elevating the whole workflow.
Independent tasks can continue, while `--fail-on-skip` or CI fails unmet work.

Resolve authorization for the named scope, then retry. Do not bypass policy or
create passwordless sudo rules just to clear a failure.
[Winget installer refusals](windows-and-wsl.md#packages) are different: they count as
failed package items, not only strict-mode skips.

## Packages do not install

Identify the task: `packages`, `paru`, or `aur-packages`. Native Linux packages
need pacman; there is no apt/dnf provider. Windows uses winget. AUR packages
need Arch and a healthy Paru.

```bash
dotfiles update --only packages --dry-run --verbose
```

A failed installed-package query is not an empty machine. Fix the provider
failure before retrying. Check the exact configured ID, active category, and
authorization reason.

`--only packages` does not include AUR entries. `--only aur-packages` assumes
its omitted prerequisites are satisfied unless you add `--with-deps`.
Local recipe or pin failures belong in
[Local package recipes](local-package-recipes.md#logs-and-troubleshooting).

**Applying missing Arch packages runs `pacman -Syu --needed --noconfirm` and can
upgrade the whole system.** Do not use apply as a harmless diagnostic.
For Windows scope, UAC, and policy failures, see [Windows packages](windows-and-wsl.md#packages).

## Paru bootstrap reports an incomplete Rust/Cargo prerequisite

These read-only probes separate a missing package, broken executable, and
incomplete build toolchain:

```bash
pacman -Q paru
/usr/bin/paru --version
cargo --version
```

Paru bootstrap needs Git, makepkg, sudo, and a **working** Cargo. A rustup proxy
without a selected toolchain can exist on PATH but fail `cargo --version`.

Provision the complete Arch build prerequisites using the distribution's
full-upgrade workflow, or configure rustup if that is your intended toolchain
manager. Do not repair this through partial library upgrades.

A missing `libalpm.so.*` usually means the helper needs rebuilding against the
current system libraries. Dotfiles checks the installed package and canonical
executable, rebuilds a broken helper, and validates it again. Under
`install-arch`, these probes run inside the target chroot; repairing only the
live ISO does not repair the installed system.

## Agent package update does not run or fails

```bash
dotfiles update --only agent-packages --dry-run --verbose
```

Confirm that the linked fragments exist, APM is available when native planning
needs it, and the named authentication requirement is met. Review dependency
expansion before adding `--with-deps`.

If the generated manifest needs changing, preview reports that write and the
delegated update instead of planning against stale input. With a current
manifest it delegates to `apm update -g --dry-run`.

Apply updates dependencies when their declared version rules allow it, then
installs local content and repairs deployments even when no version changed.
Edit tracked fragments/plugins, not generated manifests or installed plugin
copies. See [AI agent configuration](ai-agent-configuration.md).

## Optional analyzers are not running

| Tool state | Check result |
|---|---|
| No `shellcheck` | ShellCheck skipped |
| No `apm` | Local APM pack validation skipped |
| No `pwsh` | PSScriptAnalyzer skipped, even if Windows PowerShell exists |
| `pwsh` exists, module absent | PSScriptAnalyzer failed |

Strict runs (`--fail-on-skip` or CI) fail on these skips. Install the
prerequisites for the checks you intend to run, reopen the shell if PATH changed,
and retry. Filtering a check out is not evidence it passed.
See [CLI validation](testing.md#cli-validation).

## A symlink cannot be created on Windows

Check permission, target conflicts, and source checkout separately:

- File links need Developer Mode or suitable elevation. Directory links can
  fall back to junctions.
- Regular files and empty directories can be replaced without backup;
  nonempty directories are not recursively removed.
- A Git symlink checked out as ordinary text must be repaired in the source
  checkout. Elevating the home-link task does not fix it.

See [Developer Mode and symlinks](windows-and-wsl.md#developer-mode-and-symlinks) for
inspection commands and a preview including Developer Mode.

## systemd changes are not visible

First check whether the configured unit is **user** or **system** scope.
Bare unit names default to user scope. Substitute the actual unit name:

```bash
systemctl --user status example.service --no-pager
journalctl --user -u example.service -n 50 --no-pager
```

For a system unit, omit `--user`. Check that packages, linked unit files, and
executable scripts exist. Ordering after those tasks does not mean every
unrelated failure in them blocks the service task.

Without a user manager, including during Arch chroot provisioning, dotfiles
enables user units offline rather than starting them. Log in normally to test
runtime behavior. In WSL, systemd must be running or degraded for unit
configuration to proceed.

For first-login extension work deferred by `install-arch`, inspect:

```bash
journalctl --user -u dotfiles-first-login.service -n 50 --no-pager
```

Do not restart the whole desktop as the first diagnostic.
See [Arch desktop](arch-desktop.md#session-and-services) for session/service context.

## GNOME Keyring does not unlock at login

The PAM fragments are selected for **Arch + desktop**. Preview them with:

```bash
dotfiles update --only system-files --dry-run --verbose
```

This task can cover other selected system files too. Review before applying;
do not hand-edit unrelated PAM rules.

After an authorized apply, save work, fully log out of the TTY session, and log
back in. Restarting Hyprland alone does not repeat PAM login. Inspect the keyring:

```bash
busctl --user get-property \
  org.freedesktop.secrets \
  /org/freedesktop/secrets/collection/login \
  org.freedesktop.Secret.Collection Locked
```

`b false` means unlocked. If the keyring password differs from the login
password, change it through the keyring's normal password management.
Passwordless autologin supplies no password for PAM to unlock it.

## WSL settings did not change

Run the Linux CLI **inside the distribution**. Confirm `system-files` applied
the WSL-selected settings; a dry run does not change `/etc/wsl.conf`.

After saving all WSL work, restart from Windows:

```powershell
wsl --shutdown
```

This terminates **all running distributions and their processes**, not just
the current one. Reopen the intended distribution afterward.
See [WSL setup](windows-and-wsl.md#wsl).

## Remove did not restore machine defaults

This is expected. Remove copies selected home-link content into real files,
removes matching managed hooks/launchers, and runs active overlay removal
scripts. It does not restore a machine snapshot or uninstall everything that
update can configure.

Use the original profile and overlay. Keep the checkout until copying
succeeds, and remember that copies contain the current source rather than your
pre-install files. See [Remove](commands-and-usage.md#remove) for what remains.
