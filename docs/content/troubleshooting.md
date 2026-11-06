---
title: Troubleshooting
description: Diagnose failed or skipped tasks, bootstrap and package problems, configuration errors, and platform-specific symptoms.
---

# Troubleshooting

**Read the failed run before retrying a full update.** A retry can update the
CLI, sync repositories, or repeat external work. Earlier successful changes
are not rolled back when a later task fails.

1. Copy the log-viewing command printed after the failure. It selects the run
   by its stable `--id`, not by its position in history.
2. Check its profile, platform, overlay, and requested tasks.
3. Find the first failed or skipped task. For blocked tasks, check the
   prerequisite named in the reason.
4. Fix that cause, preview a focused rerun, then apply it when ready.

Examples assume you are in the intended checkout with dotfiles on PATH.
If the launcher is uncertain, substitute an existing `./bin/dotfiles` or
`.\bin\dotfiles.exe`. A wrapper can download or build the CLI before showing help.

## Read the failed run

```bash
dotfiles log --list
dotfiles log --task symlinks --verbose
```

The second command examines the `symlinks` task in the newest run. Replace
`symlinks` with the selector for the task that failed. For an older failure,
pass its run ID from history to `--id`; IDs stay stable when new runs are added.

Read the failed command's diagnostics, not just the task summary or an
“update available” notice. If the run only reports a restart or elevation,
follow its link to the child process's log.

The [Logs reference](cli-reference.md#logs) explains verbose/raw output,
missing output, and unfinished runs.

If logging failed, check free space and permissions at the
[log location](cli-reference.md#log-locations). Sanitize private paths, configuration,
and external tool output before sharing logs.

## Recover from a partial update

Completed changes remain when another task fails. Read the failed run using
the exact log command printed at the end of that run; it includes a stable ID.
Fix the reported cause before retrying.

For example, if the systemd task failed because a selected user unit file was
missing, restore its tracked source and check its symlink declaration. On an
already configured machine, inspect a retry with prerequisites included:

```sh
dotfiles list --graph update --only systemd --with-deps --skip repository
dotfiles update --no-repo-update --only systemd --with-deps --dry-run
```

The graph may include package installation and other setup work. Review the
whole selection, then remove `--dry-run` to retry. Use `--only systemd` without
`--with-deps` only when its prerequisites are already satisfied. Keep the same
profile and overlay as the failed run.

The retry inspects current state and leaves already-correct items alone; it
does not replay or undo the earlier run. Review its summary and logs again.
If earlier unrelated tasks also failed, address those separately or preview a
full update after resolving their causes. Avoid deleting managed trees or
running remove as a general failure repair.

## Startup and configuration

### Wrapper download fails

| Error | Check |
|---|---|
| Cannot resolve a release repository | The checkout's GitHub `origin` or configured release publisher |
| Latest release/download unavailable | DNS, proxy, HTTPS access, rate limits, and whether that publisher has releases |
| Unsupported architecture | Published assets support Linux x86-64/AArch64 and Windows x86-64 |
| Missing checksum or checksum mismatch | The matching release asset/checksum file; do not execute the failed download |
| Provenance verification failed | `gh`, authentication, network access, and the verification error |
| Binary cannot execute | OS/architecture, Linux executable permission, and local file-access policy |

Forks do not silently fall back to this project's binaries. A fork without
releases needs a deliberate publisher override or a source build; the
[fork instructions](cli-reference.md#using-a-fork) show both choices.

Bootstrap warns and continues if GitHub CLI is absent. If `gh` is present,
verification failure blocks it; self-update also refuses unverifiable updates.
Do not bypass checksums or set `DOTFILES_SKIP_ATTESTATION=1` as a routine fix.
See [Build provenance verification](security-and-trust.md#build-provenance-verification).

Wrappers clean failed downloads. Do not delete the checkout or all of `bin/`.
If one stale binary must be moved aside, first confirm no run is using it.

### CLI does not self-update

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

On Linux, self-update keeps the installed executable available until the
verified replacement is renamed into place. It retains the previous executable
as `bin/.dotfiles.old` for rollback if the new binary fails its startup test.
An interrupted replacement can leave either version installed; check
`bin/dotfiles --version` before retrying. Windows moves the old executable to
`bin/.dotfiles-old.exe` before installing the replacement, so an interruption
between those steps can leave `bin/dotfiles.exe` absent. If that happens, confirm
no update is running and rename the backup back to `bin/dotfiles.exe`.

For authentication, release discovery prefers nonempty `GH_TOKEN`, then
`GITHUB_TOKEN`, then an unattended saved GitHub CLI login. An expired explicit
token can override a working login; correct or unset it. Denied credentials
are not silently retried anonymously.

Anonymous requests share a lower GitHub quota with other users of the same
public IP. Respect a reported retry deadline instead of repeatedly retrying.
Normal saved authentication can be set up with `gh auth login`.
For Linux keyring-only credentials, follow the
[automatic verification authentication guidance](security-and-trust.md#build-provenance-verification).

<details>
<summary>Legacy updater recovery</summary>

These recovery steps apply to an installed binary that rejects the current
`vYYYY.MM.DD-N` release names or blocks on a desktop-keyring dialog during
automatic verification. They are not normal setup steps: the current updater
supports those release names and disables interactive keyring access.

For an unrecognized release name, preserve the affected executable and bootstrap
again with the checkout's wrapper. Confirm that no run is using it before
moving it. Never bypass verification to force an upgrade.

For a hidden credential dialog, including under WSL, provide authentication
through the [supported credential sources](security-and-trust.md#build-provenance-verification).
A [source build](contributing.md#source-builds) avoids the installed updater.
To temporarily disable a release check instead, use the
[lifecycle control](commands-and-usage.md#control-what-advances);
it does **not** disable the rest of update.

</details>

### Cargo build fails

Use the [build prerequisites](contributing.md) and read the first compiler or
linker error, not just the wrapper's final status.

Use the [source-build command](contributing.md#source-builds) to build and print
a version without applying configuration. Do not copy an arbitrary old
executable into `bin/` to hide a build failure.

### No profile selected

```bash
dotfiles list --profile base
git config --local --get dotfiles.profile
```

Use exactly `base` or `desktop`. A nonempty `DOTFILES_PROFILE` overrides the
saved Git setting, so check it if the saved value looks correct. Invalid values,
including names with surrounding spaces, cause an error instead of falling
back to the saved profile. Pass `--profile base` or `--profile desktop` to
override the environment for one run. See
[selection priority](profiles-and-categories.md#resolution-priority) for the
full order.

`list` never prompts for a profile. Update/remove/check also need an existing
selection in unattended or JSON Lines mode, with non-terminal stdin, or with
any present `CI` variable. An explicit profile overrides the saved choice
without updating it. See [Profile selection](profiles-and-categories.md#resolution-priority).

### Configuration does not parse

Start with the filename and complete diagnostic. Common causes are a missing
required file, malformed TOML, wrong value type, unsupported key/category,
conflicting targets, unsafe path, or missing source.

**Configuration loads before task filtering.** `--only symlinks` cannot bypass
malformed package configuration. Inactive categories are not a safe place to
leave invalid definitions.

Use [Configuration validation](configuration-basics.md#validation) to interpret the
finding and [CLI validation](testing.md#cli-validation) to recheck it.
`check` does not repair files, and `config-diagnostics` fails on warning-severity
diagnostics too.

### Overlay is ignored

For a sibling overlay called `dotfiles-private`, inspect it from the main
checkout:

```bash
git config --local --get dotfiles.overlay
dotfiles list --overlay ../dotfiles-private
```

Then check:

1. The selected path is the overlay root, not its `conf/` directory.
2. A higher-priority flag or environment value is not overriding the saved path.
3. Its sections match the active categories.
4. The override names the same package, destination, or setting as the base
   entry, following the [merge rules](private-overlays.md#merge-rules).
   Its category must also be active.
5. Script declarations live in overlay `conf/scripts.toml`, with script paths
   relative to that overlay root.

Missing optional files add nothing, so a wrong path can look like an empty
overlay. See [Private overlays](private-overlays.md#selection-and-persistence)
for selection, persistence, and linked-worktree requirements.

## Tasks and repository updates

### Task is missing or skipped

Inspect selection before executing more work. For an update task, pass its
selector to `--only` when viewing the graph. This example uses `systemd`:

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

### Repository is locked

Update, remove, and check share a lock, including previews. Linked worktrees
use the same common Git directory. The error identifies the lock and, when
available, the owner PID, command, and start time.

Wait for the owner or inspect it. `list` and `log` remain usable.
**Do not delete the lock file to force concurrent runs.** The process holds
the lock; leftover owner text alone does not keep it locked.

For a stuck run, follow the [interrupt procedure](commands-and-usage.md#interrupting-a-run).
After force quit, check for surviving child installers before retrying.

### Repository update fails

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

### Elevation is unavailable

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
Independent tasks can continue. With `--fail-on-skip` or in CI, a task skipped
for unavailable privileges makes the command fail.

Resolve authorization for the named scope, then retry. Do not bypass policy or
create passwordless sudo rules just to clear a failure.
[Winget installer refusals](windows-and-wsl.md#packages) are different: they count as
failed package items, not only strict-mode skips.

## Packages and tools

### Packages do not install

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
Arch package recipe or pin failures belong in
[Arch package recipes](package-management.md#logs-and-troubleshooting).

**Installing missing Arch packages can upgrade the whole system.** It runs
`pacman -Syu --needed --noconfirm`; do not use apply as a harmless diagnostic.
For Windows scope, UAC, and policy failures, see [Windows packages](windows-and-wsl.md#packages).

### Paru cannot build or start

These read-only probes separate a missing package, broken executable, and
incomplete build toolchain:

```bash
pacman -Q paru
/usr/bin/paru --version
cargo --version
```

Paru bootstrap needs Git, makepkg, sudo, and a working Cargo. A rustup proxy
without a selected toolchain can exist on PATH but fail `cargo --version`.

Provision the complete Arch build prerequisites using the distribution's
full-upgrade workflow, or configure rustup if that is your intended toolchain
manager. Do not repair this through partial library upgrades.

A missing `libalpm.so.*` usually means the helper needs rebuilding against the
current system libraries. Dotfiles checks the installed package and canonical
executable, rebuilds a broken helper, and validates it again. When provisioning
Arch in a chroot, inspect the target system; repairing only the live ISO does
not repair the installed system.

### Agent deployment fails

```bash
dotfiles update --only agent-packages --dry-run --verbose
```

Confirm that the selected source fragments exist, APM is on PATH, and any
reported authentication problem is resolved. Dotfiles reads selected sources
even if their home links are not yet installed. Review dependency expansion
before adding `--with-deps`.

If the generated manifest needs changing, preview reports that write and the
delegated update instead of planning against stale input. With a current
manifest it delegates to `apm update -g --dry-run`.

Apply updates dependencies when their declared version rules allow it, then
installs local content and repairs deployments even when no version changed.
Edit tracked fragments/plugins, not generated manifests or installed plugin
copies. See [Agent configuration](agent-configuration.md).

### Analyzers are skipped

Read the named missing tool or module in the check result, then use the
[analyzer prerequisites](testing.md#analyzer-prerequisites) to resolve it.
Reopen the shell if PATH changed and retry. Strict runs (`--fail-on-skip` or CI)
fail on unmet skips; filtering a check out is not evidence it passed.

## Platform settings

### Windows symlink fails

Check permission, target conflicts, and source checkout separately:

- File links need Developer Mode or suitable elevation. Directory links can
  fall back to junctions.
- Back up and move aside existing files, directories, or unrelated links
  that block installation.
- A Git symlink checked out as ordinary text must be repaired in the source
  checkout. Elevating the home-link task does not fix it.

See [Developer Mode and symlinks](windows-and-wsl.md#developer-mode-and-symlinks) for
inspection commands and a preview including Developer Mode.

### Service changes are not visible

First check whether the configured unit is user or system scope.
Bare unit names default to user scope. For example, inspect Quickshell with:

```bash
systemctl --user status quickshell.service
journalctl --user -u quickshell.service -b
```

Use the affected unit's name; for a system unit, omit `--user`.
The journal command shows this boot's entries. Press `q` to leave the pager.
Check that packages, linked unit files, and
executable scripts exist. Ordering after those tasks does not mean every
unrelated failure in them blocks the service task.

Without a user manager, including during Arch chroot provisioning, dotfiles
enables user units offline rather than starting them. Log in normally to test
runtime behavior. In WSL, systemd must be running or degraded for unit
configuration to proceed.

For extension installation deferred during Arch chroot provisioning, inspect:

```bash
journalctl --user -u dotfiles-first-login.service -b
```

Do not restart the whole desktop as the first diagnostic.
See [Arch desktop](arch-desktop.md#session-and-services) for session/service context.

### GNOME Keyring stays locked

The PAM fragments are selected for Arch + desktop. Preview them with:

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

### WSL settings are unchanged

Run the Linux CLI **inside the distribution**. Confirm `system-files` applied
the WSL-selected settings; a dry run does not change `/etc/wsl.conf`.

After saving all WSL work, restart from Windows:

```powershell
wsl --shutdown
```

This terminates **all running distributions and their processes**, not just
the current one. Reopen the intended distribution afterward.
See [WSL setup](windows-and-wsl.md#wsl).

## Removal leaves settings behind

This is expected. Remove copies selected home-link content into real files,
removes matching managed hooks/launchers, and runs active overlay removal
scripts. It does not restore a machine snapshot or uninstall everything that
update can configure.

Use the original profile and overlay. Keep the checkout until copying
succeeds, and remember that copies contain the current source rather than your
pre-install files. See [Remove integrations](commands-and-usage.md#remove-integrations) for what remains.
