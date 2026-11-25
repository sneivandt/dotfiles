---
title: Troubleshooting
description: Diagnose failed or skipped tasks, bootstrap and package problems, configuration errors, and platform-specific symptoms.
---

# Troubleshooting

**Start with the failed run, not another full update.** Earlier changes stay
applied, and a retry can sync repositories or repeat external installs. If the
CLI printed a `dotfiles log --id` command, copy it before you retry. If the
wrapper never started the CLI, use the terminal error instead.

The commands below assume you are in your checkout with `dotfiles` on `PATH`.
If the launcher itself is in doubt, call `./bin/dotfiles` or
`.\bin\dotfiles.exe` directly only after confirming you trust that cached
binary, since running a wrapper can download or build the CLI before printing
output.

## Find the symptom

| Symptom | Start here | First evidence |
|---|---|---|
| No CLI starts or a download is rejected | [Wrapper download fails](#wrapper-download-fails) | Terminal error and selected binary publisher |
| Wrong role or private configuration | [No profile selected](#no-profile-selected) or [Overlay is ignored](#overlay-is-ignored) | Flags, environment, and repository-local selection |
| Existing content blocks a link | [Home symlink preview](../reference/home-symlinks.md#preview-and-apply) or [Windows symlink fails](#windows-symlink-fails) | Destination type, current link target, and proposed source |
| A task is absent, skipped, or blocked | [Task is missing or skipped](#task-is-missing-or-skipped) | Failed run ID, reason, and selected graph |
| Applications do not install | [Packages do not install](#packages-do-not-install) | Package task's diagnostics, exact ID, and provider |
| Some changes applied before failure | [Recover from a partial update](#recover-from-a-partial-update) | Original run and linked child logs |
| A service or WSL setting appears unchanged | [Service changes](#service-changes-are-not-visible) or [WSL settings](#wsl-settings-are-unchanged) | Unit scope/state or the environment where the CLI ran |

## Read the failed run

Read the pinned run without a task filter first. Check its profile, overlay,
and task selection. For a `BLOCKED` row, find the prerequisite named in the
reason; its diagnostics tell you what to fix. If the run restarted or delegated
elevation, follow the linked child log before narrowing by task.

If you didn't save the run ID, look it up with `--list`. Replace `<run-id>`
and `symlinks` with the ID and failing selector:

```bash
dotfiles log --list
dotfiles log --id <run-id> --task symlinks --verbose
```

Without `--id`, the log command reads the newest run, not necessarily the
failed one. A configuration error can have a run ID even if no task started;
in that case leave off `--task`.

See the [Logs reference](../reference/cli-reference.md#logs) for raw/verbose
modes, missing output, and interrupted runs. If logging itself failed, check
disk space and permissions at the
[log location](../reference/cli-reference.md#log-locations), and sanitize
private paths and tool output before sharing any excerpt.

## Recover from a partial update

If the failure was an interrupted Git merge, follow
[Recover from an interrupted merge](../reference/dotfiles-repository.md#recover-from-an-interrupted-merge)
first; narrowing task filters will not bypass its recovery marker.

As an example, if `systemd` failed because a user unit file was missing,
restore the tracked source and check its symlink entry, then inspect and
preview a retry with prerequisites on an already-configured machine:

```sh
dotfiles list --graph update --only systemd --with-deps --skip repository
dotfiles update --no-repo-update --only systemd --with-deps --dry-run
```

Because `--with-deps` can pull in package installation or other setup tasks,
review the entire preview before dropping `--dry-run`. Omit `--with-deps` when
prerequisites are already satisfied, and keep the same profile and overlay as
the failed run.

After applying, repeat the focused preview: expect no further planned work
for the repaired task and no unresolved skips, blocks, or failures. A clean
focused retry says nothing about tasks it excluded. Preview a full update
after addressing other failures; don't use `remove` as a reset.

## Startup and configuration

### Wrapper download fails

| Error | Check |
|---|---|
| Cannot resolve a release repository | The checkout's GitHub `origin` or configured release publisher |
| Latest release/download unavailable | DNS, proxy, HTTPS access, rate limits, and whether that publisher has releases |
| Unsupported architecture | Published assets support Linux x86-64/AArch64 and Windows x86-64 |
| Missing checksum or checksum mismatch | The matching release asset/checksum file; never execute a failed download |
| Provenance verification failed | `gh`, authentication, network access, and the verification error |
| Binary cannot execute | OS/architecture, Linux executable permission, and local file-access policy |

Forks never silently fall back to upstream binaries. If your fork has no
releases, configure an explicit publisher or build from source following
[Using a fork](getting-started.md#using-a-fork).

If the wrapper reports `gh not found`, it stops without installing the
download. Install GitHub CLI and retry, or
[build from your reviewed checkout](../development/contributing.md#source-builds).
If `gh` is already installed, verify it can access the release repository.
A valid checksum does not replace
[build provenance verification](security-and-trust.md#build-provenance-verification),
so do not disable provenance checks to clear an unexplained error.

Wrappers clean up failed downloads automatically. Never delete the checkout or
wipe `bin/`; if you must move one stale binary aside, confirm no run is using
it first.

### CLI does not self-update

If no update appeared, first check which binary ran. Only `update` and
`remove` check for a release, and only from a release binary in the selected
checkout's `bin/`. Its publisher is embedded; changing `origin` or
`DOTFILES_RELEASE_REPO` won't retarget it. `DOTFILES_SKIP_SELF_UPDATE=1`
disables the check, `--dry-run` only reports an available update, and cached
metadata can be reused for ten minutes.

A release lookup failure lets the current CLI continue. A download or
verification failure stops the run: read that error before retrying.

If self-update was interrupted, wait for any active update to finish, then run
the checkout wrapper with `--version`. When it finds a pending update, it
keeps a working installed binary or restores a working backup if the installed
one won't start. That recovery needs no network. If neither starts, keep both
files and inspect the reported `bin/` directory rather than deleting it.

For GitHub API authentication, release discovery tries nonempty `GH_TOKEN`,
then `GITHUB_TOKEN`, then an unattended saved `gh` login. An expired explicit
token overrides a working saved login, so fix or unset it; denied credentials
are never silently retried anonymously.

Unauthenticated requests share a tighter IP-based rate limit. Wait for any
reported reset time instead of retrying in a loop, or authenticate with
`gh auth login`. For Linux credentials stored only in a desktop keyring, see
[Build provenance verification](security-and-trust.md#build-provenance-verification).

<details>
<summary>Legacy updater recovery</summary>

These steps apply only to older installed binaries:

- **Interrupted update with leftover backup:** Older updaters did not record an
  in-progress marker. If the main binary is missing while its backup starts,
  the wrapper runs the backup in place. Once no update is running, rename
  `bin/.dotfiles-old.exe` back to `bin/dotfiles.exe` on Windows, or
  `bin/.dotfiles.old` to `bin/dotfiles` on Linux.
- **Unrecognized `vYYYY.MM.DD-N` release names:** Keep a copy of the binary and
  re-bootstrap with the checkout's wrapper once no run is active. Never bypass
  verification to force an upgrade.
- **Hidden desktop-keyring dialog (including under WSL):** Supply credentials
  through the [supported sources](security-and-trust.md#build-provenance-verification),
  use a [source build](../development/contributing.md#source-builds), or
  temporarily skip release checks using
  [lifecycle controls](commands-and-usage.md#control-what-advances).

</details>

### Cargo build fails

Check the [build prerequisites](../development/contributing.md) and read the
first compiler or linker error rather than only the wrapper's final status.

Run the [source-build command](../development/contributing.md#source-builds) to
compile and verify the binary version without applying configuration. Avoid
copying an old binary into `bin/` to mask a build error.

### No profile selected

```bash
dotfiles list --profile base
git config --local --get dotfiles.profile
```

Profiles must be exactly `base` or `desktop` (invalid values, including stray
whitespace, fail instead of falling back). `DOTFILES_PROFILE` overrides the
saved Git setting, and `--profile` overrides both for one run without saving.
`list`, `explain`, and unattended runs (non-TTY stdin, JSON Lines, or `CI`)
never prompt for a profile. See [Resolution priority](profiles-and-categories.md#resolution-priority).

### Configuration does not parse

Start with the filename and full diagnostic message. Typical causes are a
missing required file, syntax error, wrong type, unknown key or category,
conflicting target declarations, unsafe path, or missing source file.

Configuration normally loads before task filtering. `--only symlinks` cannot
skip broken package TOML, even in an inactive category. If a **trusted
upstream already contains the fix**, a repository-only update is the
exception: preview `dotfiles update --only repository --dry-run` with no
`--skip` filters. Apply only if you're ready for the checkout and its live
linked files to advance. This won't bypass an interrupted-merge marker.

Otherwise repair the reported file and retest with `dotfiles check`. It does
not apply configuration, but writes a log and can save selections.
`config-diagnostics` fails on warnings too. See
[Configuration validation](configuration-basics.md#validation) and
[CLI validation](../development/testing.md#cli-validation).

### Overlay is ignored

To check a sibling overlay named `dotfiles-private` from your main checkout:

```bash
git config --local --get dotfiles.overlay
dotfiles list --overlay ../dotfiles-private
```

Then verify:

1. The path points to the overlay root, not its `conf/` subdirectory.
2. No higher-priority `--overlay` flag or `DOTFILES_OVERLAY` value is overriding
   the saved path.
3. The overlay's section headers match your active categories.
4. Any replacement matches the base entry's resource key under the
   [merge rules](private-overlays.md#merge-rules) and sits in an active
   category.
5. Script tasks live in the overlay's `conf/scripts.toml` with paths relative
   to the overlay root.

An existing directory without overlay declarations can look like an empty
overlay; a missing path or non-directory produces an error. See
[Selection and persistence](private-overlays.md#selection-and-persistence) for
resolution order and linked-worktree rules.

## Tasks and repository updates

### Task is missing or skipped

Check selection and dependencies before running more work (substituting your
task's selector for `systemd`):

```bash
dotfiles list
dotfiles list --graph update --only systemd
```

| What you see | What to check |
|---|---|
| Unknown selector | Use an exact selector or full label from `dotfiles list`, not a substring or package ID |
| Missing from the command | Check the right command group (`update`, `remove`, or `check`); try `dotfiles list --all` |
| `filtered` or `skipped` in the graph | Check both `--only` and `--skip` flags |
| Omitted prerequisite warning | Decide whether to include `--with-deps` |
| No normal result row | The task may already be current or inapplicable (`--verbose` shows unchanged tasks; logs also show inapplicable ones) |
| `requires …` or blocked | Resolve the named prerequisite first |
| Missing capability/tool | Install or enable that prerequisite deliberately |
| No active entries | Check your active profile and categories |

For a focused preview:

```bash
dotfiles update --only systemd --dry-run --verbose
```

Always inspect the graph before adding `--with-deps`: it can bring in package
installation, Paru bootstrap, custom recipe prep, symlinks, and the launcher
rather than narrowing your retry.

### Repository is locked

`update`, `remove`, and `check` acquire locks on the main checkout, any active
Git overlay, and your home directory (even during previews). Linked worktrees
share an account-local lock, and separate clones targeting the same home share
`~/.local/state/dotfiles/home-run.lock` (Linux) or
`~/AppData/Local/dotfiles/home-run.lock` (Windows). The lock error prints the
lock path and, when available, the owner PID, command, and start time (which
may belong to a restarted child after a repo sync).

Wait for the owning process to finish or inspect it; `list` and `log` still
work while locked. **Never delete the lock file to force concurrent runs**: the
OS lock is held by the running process, and leftover owner text does not keep a
released lock active.

If a run appears stuck, follow
[Interrupting a run](commands-and-usage.md#interrupting-a-run), and check for
surviving child installers after any force quit before retrying.

### Repository update fails

Inspect the affected main or overlay checkout:

```bash
git status --short
git branch -vv
git remote -v
```

Check remote URLs for embedded tokens before sharing this output.

| Cause | Recovery |
|---|---|
| Tracked local changes | Keep the edits. Commit or stash work you want to keep before syncing, or deliberately use the current checkout without syncing. |
| Detached HEAD | Sync is inapplicable; check out your intended branch if appropriate |
| Missing upstream | Configure an upstream branch or intentionally stay on local state |
| Local/diverged commits | Reconcile the branch manually; dotfiles never resets or rebases |
| Authentication/network failure | Inspect the failing Git command and credentials |
| Fast-forward merge failure | Protect local content, including colliding untracked files |

Untracked files can still block a fast-forward even after readiness checks
pass, and a failed fast-forward stops the rest of the run. Git may write files
before advancing the branch (including files linked into your home), and if the
main repo advanced before the overlay failed, the main repo stays at the new
commit. A recovery marker in either checkout blocks all subsequent `update` and
`remove` runs, including previews and `--only` retries.

Follow [Recover from an interrupted merge](../reference/dotfiles-repository.md#recover-from-an-interrupted-merge)
before clearing the marker. If the launcher itself is broken, use Git directly
or a verified cached binary instead of bootstrapping from the damaged checkout.
If Git reports `index.lock`, leave it alone unless you've confirmed it is
orphaned; it is Git's own lock, not the dotfiles recovery marker.

Use `--no-repo-update` when you want to apply an unmarked checkout without
syncing (it doesn't discard edits or disable other network tasks). Seeing
"Repository synced · restarting to load configuration" is normal: the child
reloads the updated config while the parent holds the lock.

### Elevation is unavailable

| Reason | Meaning |
|---|---|
| `elevation declined` | Windows UAC prompt was cancelled |
| `elevation unavailable in a non-interactive session` | No interactive UAC prompt path is available |
| `sudo credentials unavailable` | Linux `sudo` was missing, denied, or could not be primed |
| `sudo credentials unavailable in a non-interactive session` | No usable cached or passwordless `sudo` authorization |
| `requires <task>` | A blocking prerequisite did not complete |
| `ran in elevated session` | Work ran in the elevated child; inspect the linked child log |

Windows delegates only selected machine tasks, and Linux primes `sudo` for
privileged commands; neither requires running the entire session elevated.
Independent tasks continue normally, though `--fail-on-skip` and CI turn
privilege skips into command failures.

Restore authorization for the affected scope and retry rather than relaxing
policy or adding passwordless `sudo` rules just to silence an error. Note that
[WinGet installer refusals](windows-and-wsl.md#packages) count as failed
package items rather than skips.

## Packages and tools

### Packages do not install

First identify which task failed: `packages`, `paru`, or `aur-packages`. Native
Linux packages require pacman (there is no `apt` or `dnf` provider), Windows
uses WinGet, and AUR packages require Arch with a healthy Paru helper.

```bash
dotfiles update --only packages --dry-run --verbose
```

If querying installed packages fails, fix the package-manager error before
retrying (dotfiles never treats a failed query as an empty machine). Check the
exact package ID, active category, and elevation status.

Remember that `--only packages` excludes AUR entries, and `--only aur-packages`
assumes its prerequisites are already met unless you pass `--with-deps`. For
custom Arch recipe or pin errors, see
[Package management troubleshooting](package-management.md#logs-and-troubleshooting).

**Installing missing Arch packages can upgrade the entire system** via
`pacman -Syu --needed --noconfirm`, so avoid running apply just as a quick test.
For Windows scope, UAC, and policy failures, see
[Windows packages](windows-and-wsl.md#packages).

### Paru cannot build or start

Run these read-only checks to tell a missing package, broken binary, and
incomplete Rust toolchain apart:

```bash
pacman -Q paru
/usr/bin/paru --version
cargo --version
```

Bootstrapping Paru requires Git, `makepkg`, `sudo`, and a working `cargo` (a
`rustup` shim with no active toolchain can exist on `PATH` and still fail
`cargo --version`).

Install complete Arch build prerequisites through a full system upgrade, or
configure your intended `rustup` toolchain; never attempt partial library
upgrades.

A missing `libalpm.so.*` error means Paru needs a rebuild against the updated
system libraries. Dotfiles detects a broken helper, rebuilds it, and verifies
the rebuilt binary. Inside an Arch chroot, make sure you inspect and repair the
target root rather than the live ISO environment.

### Agent deployment fails

```bash
dotfiles update --only agent-packages --dry-run --verbose
```

Verify that your selected fragments exist, `apm` is on `PATH`, and any reported
authentication issue is resolved. Check the dependency graph before adding
`--with-deps`. Edit tracked fragments and plugins rather than generated
manifests or deployed files; see
[Troubleshoot deployment](agent-configuration.md#troubleshoot-deployment) for
merge, preview, and integration checks.

### Analyzers are skipped

Look at the missing tool or module named in the check output and follow
[Analyzer prerequisites](../development/testing.md#analyzer-prerequisites).
Open a new shell if `PATH` changed and rerun. Under `--fail-on-skip` or in CI,
skipped analyzers fail the run; excluding a check with `--skip` doesn't count
as passing it.

## Platform settings

### Windows symlink fails

Separate permission, conflict, and Git checkout issues before changing files:

- Enable Developer Mode, then retry file links from your normal user's
  terminal (home links are never UAC-delegated; directory links can fall back
  to junctions).
- Inspect the destination and proposed source first. Back up an existing file
  or directory before moving it aside; for an obsolete link or junction, move
  only the link, not the contents it points to.
- Repair plain-text Git symlink placeholders in the source checkout, since
  turning on Developer Mode later won't convert already-checked-out text stubs.

See [Developer Mode and symlinks](windows-and-wsl.md#developer-mode-and-symlinks)
for inspection commands and a preview that includes Developer Mode.

### PowerShell profile does not load

Inspect the profile path inside the exact PowerShell edition and terminal host
that isn't loading it:

```powershell
$PROFILE.CurrentUserCurrentHost
Test-Path -LiteralPath $PROFILE.CurrentUserCurrentHost
Get-ExecutionPolicy -List
```

Compare that path against the
[managed profile destinations](windows-and-wsl.md#powershell-configuration),
noting that OneDrive-redirected Documents folders and hosts like VS Code can
differ. Also check whether the shell launched with `-NoProfile` and read any
startup error (an existing file alone doesn't prove it executed).

If execution policy blocks the profile or wrapper, follow your organization's
policy and PowerShell's error message rather than relaxing machine-wide policy.
Adjust any mismatched destination in `conf/symlinks.toml`, preview `symlinks`,
and edit preferences in the tracked profile source rather than a copy.

### Service changes are not visible

Confirm whether the unit is user-scoped or system-scoped (bare names default to
user scope). For example, to inspect Quickshell:

```bash
systemctl --user status quickshell.service
journalctl --user -u quickshell.service -b
```

Replace `quickshell.service` with your unit, and omit `--user` for system
units. The journal command shows logs from the current boot (press `q` to quit
the pager). Verify that required packages, linked unit files, and helper
scripts are present; ordering after those tasks doesn't guarantee every
unrelated failure in them blocked `systemd`.

When no systemd user manager is running (such as during Arch chroot
provisioning), dotfiles enables user units offline without starting them; log
in normally to verify runtime behavior. Inside WSL, systemd must be running or
degraded before unit management applies.

To inspect VS Code extension installation deferred during Arch chroot setup:

```bash
journalctl --user -u dotfiles-first-login.service -b
```

Avoid restarting your entire desktop as a first troubleshooting step. See
[Session and services](arch-desktop.md#session-and-services) for how desktop
units are wired.

### GNOME Keyring stays locked

The PAM fragments that unlock GNOME Keyring belong to `arch-desktop`. Preview
them with:

```bash
dotfiles update --only system-files --dry-run --verbose
```

Because `system-files` can cover other `/etc` files as well, review the full
preview before applying, and avoid hand-editing unrelated PAM rules.

After applying, save your work, log completely out of your TTY session, and log
back in (restarting Hyprland alone won't re-run PAM login). Verify unlock
status with:

```bash
busctl --user get-property \
  org.freedesktop.secrets \
  /org/freedesktop/secrets/collection/login \
  org.freedesktop.Secret.Collection Locked
```

`b false` confirms the keyring is unlocked. If the keyring password doesn't
match your login password, update it in the keyring settings; passwordless
autologin supplies no password for PAM to unlock the keyring.

### WSL settings are unchanged

Make sure you ran the Linux CLI **inside the WSL distribution** and applied
`system-files` without `--dry-run` so `/etc/wsl.conf` was actually updated.

Save work across every running distribution, then restart WSL from Windows:

```powershell
wsl --shutdown
```

This shuts down **all running WSL distributions and their processes**, not just
one. Reopen your distribution afterward; see [WSL setup](windows-and-wsl.md#wsl).

## Removal leaves settings behind

That behavior is intentional. `dotfiles remove` materializes selected home
symlinks into ordinary files, removes matching managed Git hooks and launchers,
and runs active overlay removal scripts; it does not restore a pre-install
snapshot or uninstall packages and settings.

Run `remove` with your original profile and overlay while the checkout is still
present, and remember that materialized files hold the current checkout's
content rather than pre-dotfiles originals. See
[Remove integrations](commands-and-usage.md#remove-integrations) for the full
breakdown of what stays behind.
