# Troubleshooting

**Read the failed run before starting another full install.** A retry can
change the CLI binary, synchronize repositories, or repeat external work.
Successful earlier changes are not rolled back when a later task fails.

1. Copy the exact `dotfiles log --id ... -v` command printed after the failure.
   It keeps selecting that run even after other commands execute.
2. If no hint is available, use `dotfiles log --list`, then select the run.
3. Identify the first failing or unmet task, not just its blocked dependents.
4. Confirm the header's profile, platform, and overlay and the intended
   checkout. Fix that cause before retrying.

The examples below assume a bootstrapped CLI on PATH and the current directory
is the intended repository root. Replace `dotfiles` with an **existing**
`./bin/dotfiles` or `.\bin\dotfiles.exe` when PATH is uncertain.
A wrapper invocation can download/build before displaying help or a preview.

| Symptom | Start here |
|---|---|
| Failure before the CLI header | [Bootstrap](#the-wrapper-cannot-find-or-download-the-binary), [profile](#no-profile-can-be-selected), [configuration](#configuration-does-not-parse) |
| Task absent, current, skipped, or blocked | [Task selection](#a-task-does-not-run) |
| Another run owns the repository | [Run lock](#another-run-holds-the-repository-lock) |
| Wrong files or private settings | [Overlay selection](#an-overlay-appears-to-be-ignored) |
| A command ran but output is unclear | [Logs and partial results](#reading-logs-and-partial-results) |
| Desktop/service settings not active yet | [systemd](#systemd-changes-are-not-visible), [Keyring](#gnome-keyring-does-not-unlock-at-login), [WSL](#wsl-settings-did-not-change) |

## Reading logs and partial results

```bash
dotfiles log --list
dotfiles log --task symlinks --verbose
```

The second command filters the **newest** run. Add its real `--id` from history
when investigating an earlier failure. `--command install` filters history
before applying a numeric index; update runs are separate from install runs.

Failed-command stdout/stderr is visible without `--verbose`; verbose adds other
diagnostics. Task summaries prefer concrete errors over notices such as
“update available.” Use the captured command record rather than assuming the
first printed notice caused the failure. Successful stdout is normally not
retained, and `--raw` cannot recover omitted output.

`unfinished` means no finish record was written, not necessarily failure: the
process may still be active. Restarted and elevated child runs have separate
logs linked by parent ID. Inspect the child if the parent only reports
delegation/restart.

When persistent logging is unavailable, the CLI warns and cannot offer a
reliable log hint. Check the log directory's permissions and available space;
see [log-directory precedence](USAGE.md#logs).
Sanitize paths, private data, and external tool output before sharing a log.

## The wrapper cannot find or download the binary

| Reported failure | Check before retrying |
|---|---|
| Latest tag cannot be resolved / download failed | GitHub HTTPS access, DNS/proxy configuration, rate limiting |
| Unsupported architecture / no matching asset | Release assets support Linux x86-64/AArch64 and Windows x86-64 |
| Missing checksum / checksum mismatch | Matching asset and release checksum file; do not execute the failed download |
| Provenance verification failed | `gh` availability/authentication, network access, and the verification error |
| Binary cannot be executed | Correct OS/architecture, executable permissions on Linux, file-access policy |

The wrappers remove failed downloads; do not start by deleting the checkout or
all of `bin/`. If a known stale/partial binary remains, preserve it separately
and retry the wrapper only after confirming no run is using it.

Initial bootstrap warns and continues if `gh` is absent. If `gh` is available,
failed provenance verification blocks bootstrap. Self-update is stricter:
an unverifiable update is not installed. Do not “fix” this by disabling
checksums or setting `DOTFILES_SKIP_ATTESTATION=1`.
See [Build provenance verification](SECURITY.md#build-provenance-verification).

A source build is an alternative when build prerequisites are present. These
commands build and show the version; they do not install configuration:

```bash
./dotfiles.sh --build --version
```

```powershell
.\dotfiles.ps1 --build --version
```

They still write build artifacts and may download Cargo dependencies.

## The binary never self-updates

`--version`, help, `list`, `log`, and `check` do not trigger CLI self-update.
Before replacing a binary, distinguish expected behavior from a failed update:

- The binary must run from the selected checkout's `bin/`, not a Cargo output
  directory. `--root` pointing to another checkout can therefore affect the
  self-update decision.
- `DOTFILES_SKIP_SELF_UPDATE=1` disables the check.
- Development/non-release versions do not use the date-tag update path.
  Current releases use `vYYYY.MM.DD-N`.
- A dry run reports an available update but does not replace the binary.
- A fresh release cache can delay another lookup by up to one hour. Linux
  cache entries also have to match the current boot.
- Failed release lookup leaves the existing binary usable; inspect diagnostics
  for API rate limits, DNS, or proxy failures.

If an old binary predates support for the current release-tag format, its
updater may never recognize a new release. After confirming the executable path
and saving any needed binary, move only that checkout's `bin/dotfiles` or
`bin\dotfiles.exe` aside and invoke its wrapper to bootstrap again. Do not
remove unrelated files or bypass verification.

## Cargo build fails

Use the [Contributing](CONTRIBUTING.md) prerequisites and the canonical build
commands in [Testing](TESTING.md). Read the first compiler/linker failure,
not just the wrapper's final exit status.

Wrapper builds use `dev-opt` and execute Cargo's reported artifact. With a
custom `CARGO_TARGET_DIR` or target triple, looking only in `cli/target/debug`
can inspect the wrong binary. Do not copy an arbitrary stale executable to
`bin/` to silence a build failure.

## Paru bootstrap reports an incomplete Rust/Cargo prerequisite

The task checks the installed package with `pacman -Q paru` and the actual
target executable with `/usr/bin/paru --version`. These read-only probes can
distinguish “missing” from a library/loader failure:

```bash
pacman -Q paru
/usr/bin/paru --version
cargo --version
```

It checks `git`, `makepkg`, `sudo`, and a working Cargo before cloning/building.
An installed rustup proxy without a selected toolchain fails `cargo --version`;
merely finding `cargo` on PATH is not enough.

For a new Arch machine, deliberately provision the distribution-managed
`base-devel`, `git`, `rust`, and `sudo` prerequisites using Arch's full-upgrade
workflow, or configure a working rustup toolchain if that is your chosen
toolchain manager. Do not mix a partial library upgrade with a stale AUR helper.

A missing `libalpm.so.*` can mean Paru needs rebuilding against current system
libraries, not that dotfiles needs a different PATH. The task rebuilds a broken
helper and validates it again. Under `install-arch`, dotfiles runs in the target
chroot, so fixing only the live ISO's Cargo or Paru does not repair the target.

## No profile can be selected

Use an explicit profile for an unattended or diagnostic invocation:

```bash
dotfiles list --root . --profile base
git config --local --get dotfiles.profile
```

Precedence is CLI, nonempty `DOTFILES_PROFILE`, local `dotfiles.profile`, then
an interactive prompt. The first chosen value must be exactly `base` or
`desktop`; a typo in the environment does not fall back to saved configuration.
Empty environment values are ignored; whitespace around a name is not.

`list` never opens the profile prompt. Engine commands also require an existing
selection with non-terminal stdin, `--non-interactive`, or any present `CI`
variable (even `CI=false`). An explicit `--profile` overrides a saved choice
without updating it. See [Profiles](PROFILES.md#resolution-priority).

## Configuration does not parse

Read the complete filename and diagnostic before editing. Common causes are:

- A missing required main `conf/*.toml` file or malformed TOML.
- A misspelled/unsupported key or category.
- A value using the wrong field type.
- Conflicting desired state, an unsafe path, or a missing source.
- An invalid main or overlay contribution.

Configuration loads before task filtering, including for `list`, so
`--only symlinks` cannot bypass malformed package configuration.
Inactive category sections are not a safe place to leave malformed data;
validation also checks source definitions outside the active role.

Use [CLI validation](TESTING.md#cli-validation) and the
[configuration reference](CONFIGURATION.md#validation) to narrow the finding.
`check` does not repair files. Its `config-warnings` task treats reported
configuration diagnostics as failure, including warning-severity ones.

## An overlay appears to be ignored

From the intended main checkout, inspect saved state and explicitly discover
with the real overlay root. For example, if it is the sibling directory
`../dotfiles-private`:

```bash
git config --local --get dotfiles.overlay
dotfiles list --root . --profile desktop --overlay ../dotfiles-private
```

Check the following in order:

1. The path is the **overlay repository root**, not its `conf/` directory.
2. CLI `--overlay` overrides nonempty `DOTFILES_OVERLAY`, which overrides the
   saved local Git value.
3. Its sections match the active categories.
4. Lists append main then overlay contributions; an overlay is not a generic
   “last value wins” replacement layer. Conflicting entries need correction.
5. `scripts.toml` exists in the overlay only, and script paths are relative to
   that overlay root.

Missing optional overlay files contribute nothing. A misspelled selection can
therefore look like an empty overlay rather than an override failure.
`list` does not persist the path, but an explicit overlay on update, check,
or remove **does**, even in dry-run.

An explicit linked-worktree overlay asks for confirmation and is rejected
without a usable interactive terminal. Use a stable primary checkout for
unattended work rather than relying on a transient worktree.
See [Overlays](CONFIGURATION.md#overlays).

## A task does not run

First discover its exact selector and inspect selection without executing it:

```bash
dotfiles list --root . --profile desktop
dotfiles list --root . --profile desktop --graph update --only systemd
```

| Observation | Likely cause / next step |
|---|---|
| Unknown selector | Use the stable selector or full normalized label, not a substring or package name |
| Not in this command | Check the `COMMANDS` column; update and check have different task sets |
| `filtered` / `skipped` in graph | Inspect both `--only` and `--skip` |
| Prerequisite warning | `--only` omitted a blocking dependency; decide whether to include `--with-deps` |
| No normal console row | It can be current or inapplicable; inspect verbose output/logs |
| `requires …` / blocked | Fix the named predecessor first |
| Skipped with a missing tool/capability | Install/configure that prerequisite deliberately, then retry |
| No active entries | Check role/category selection, not just the task list |

For a focused preview after identifying the cause:

```bash
dotfiles update --root . --profile desktop --no-repo-update --only systemd --dry-run --verbose
```

Add `--with-deps` only after reviewing how much it expands the run. It can add
packages, AUR setup, symlinks, and permissions.
Use `update --only apm` to scope the run when the intended work is
advancing eligible APM refs.

## A symlink cannot be created on Windows

Distinguish missing capability, a conflicting target, and a bad source checkout:

- Developer Mode must be available or file-link creation needs elevation.
- An unrelated home file can be replaced without backup; a nonempty directory
  is not recursively removed. Preserve its contents before resolving it.
- If Git recorded the source as a symlink but checked it out as ordinary text,
  repair the source checkout after enabling symlink support. Rerunning link
  creation alone cannot fix that placeholder.

Preview Developer Mode with the link task:

```powershell
dotfiles update --root . --profile desktop --no-repo-update --only symlinks --with-deps --dry-run --verbose
```

See [Windows symlinks](WINDOWS.md#developer-mode-and-symlinks) for read-only
checkout inspection. Do not elevate the entire workflow to hide a source or
target conflict.

## GNOME Keyring does not unlock at login

The PAM fragments are selected for **Arch + desktop**. Preview their task:

```bash
dotfiles update --root . --profile desktop --no-repo-update --only system-files --dry-run --verbose
```

If the preview shows needed PAM changes, review them before deliberately
applying that task. It can also manage other selected system files. Do not
hand-edit unrelated PAM rules or assume restarting Hyprland repeats PAM login.

After applying, save work and fully log out of the TTY session, then log back
in. Inspect the login collection:

```bash
busctl --user get-property \
  org.freedesktop.secrets \
  /org/freedesktop/secrets/collection/login \
  org.freedesktop.Secret.Collection Locked
```

`b false` means unlocked. A locked collection can have a password different
from the Unix login password; update it through the keyring's normal password
management. Passwordless autologin supplies no password for PAM to unlock it.

## A task was skipped because elevation was unavailable

| Reason | Meaning |
|---|---|
| `elevation declined` | Windows UAC consent was dismissed |
| `elevation unavailable in a non-interactive session` | No supported prompt path; run in a suitable interactive terminal if authorized |
| `sudo credentials unavailable` | Linux sudo was absent, denied, or could not be primed |
| `sudo credentials unavailable in a non-interactive session` | Unattended Linux run lacked usable cached/passwordless credentials |
| `requires <task>` | A blocking predecessor was left unmet |
| `ran in elevated session` | Not a missing change: inspect the separate child run |

On Windows, only selected privileged tasks are delegated to a short-lived
child. On Linux, the CLI primes sudo credentials before tasks; it does not
move the whole run into a Windows-style elevated child.
Independent work can continue after unavailable elevation. `--fail-on-skip`
or CI turns unmet elevation into failure.

Resolve authorization for the named work, then retry that scope or the normal
workflow. Do not create passwordless sudo rules or bypass organizational policy
merely to make a run green. See [Windows elevation](WINDOWS.md#elevation).

## Another run holds the repository lock

Update, remove, and check—including previews—share a repository
lock. Linked worktrees use the same common Git directory. The error identifies
the lock and, when readable, the owner PID, command, and start time.

Wait for the owner or inspect it before interrupting it. `list` and `log`
remain usable. **Do not delete the lock file to force concurrent runs**:
the live lock is held by the process, not inferred from file existence.
Owner text left after a completed process does not itself prevent a new lock.

If a run seems stuck, inspect its active command/log and use the
[interrupt procedure](USAGE.md#interrupting-a-run). After force quit, check for
still-running child installers before retrying; partial changes are not undone.

## Repository update fails

Use read-only Git inspection in the affected main or overlay checkout:

```bash
git status --short
git branch -vv
git remote -v
```

Do not share remote URLs without checking for embedded credentials.

| Cause | Behavior / recovery |
|---|---|
| Tracked local changes | Synchronization is skipped; preserve and resolve them intentionally |
| Detached HEAD | Repository task is inapplicable |
| No upstream | Configure the intended upstream or intentionally use the current checkout |
| Local-only/diverged commits | CLI will not reset/rebase them; reconcile the branch yourself |
| Fetch authentication/network error | Inspect the failing repository and command log; transient fetch errors are retried |
| Fast-forward merge failure | Read Git's error and protect worktree content, including untracked collisions |

The readiness check ignores untracked files; that does not guarantee a merge
cannot collide with them. Main and overlay updates are not a single
transaction.

To intentionally apply local desired state without attempting synchronization,
use `--no-repo-update`. Preview first. Do not discard user changes merely to
clear a skipped-task count.

`Repository synced · restarting to load configuration` is normal after content
changes: the child reloads both static and overlay tasks, omits another
repository update, and keeps the parent's lock.

## Packages do not install

Determine which task failed: `packages`, `paru`, or `aur-packages`.
The regular Linux provider looks for pacman; there is no apt/dnf adapter.
Windows uses winget. AUR work requires Arch and a healthy target-system Paru.

```bash
dotfiles update --root . --profile desktop --no-repo-update --only packages --dry-run --verbose
```

A failed installed-package query is not an empty inventory. Correct the
provider or its output/error before retrying. Check the exact configured ID,
active category, available provider, and elevation reason. `--only packages`
does not select AUR entries; selecting AUR alone also assumes bootstrap is
already satisfied unless `--with-deps` is used.

**Applying missing Arch packages uses `pacman -Syu --needed --noconfirm`** and
can upgrade the wider system. Schedule that transaction deliberately rather
than retrying it as a harmless diagnostic.

Windows scope/UAC/policy skips are explained in
[Windows packages](WINDOWS.md#packages). Elevation does not fix an invalid
package ID, broken inventory output, or a prohibited installer.

## APM update does not run or fails

```bash
dotfiles update --root . --profile desktop --no-repo-update --only apm --dry-run --verbose
```

Update previews configuration convergence and pin advancement. Apply uses
`apm update -g --yes`, without a preceding `apm install -g` pass.

- If symlinks were filtered out, confirm the fragment inputs are present or
  preview `--with-deps` expansion.
- If `apm` is missing, provide it deliberately. Update preview can
  describe planned APM work without it; update preview needs the executable.
- Authentication failures can be unmet/skipped work. Check the named
  authentication requirement without publishing credentials.
- If the generated manifest would change, preview reports the write and
  delegated update instead of asking APM to plan against stale input.
- If the generated manifest is current, update preview delegates to
  `apm update -g --dry-run`.

Edit source fragments, not the generated manifest or deployed plugin tree.
Use [APM](APM.md) for target-specific support and ownership.

## Optional analyzers are not running

| Tool state | Check outcome |
|---|---|
| No `shellcheck` | Shellcheck skipped |
| No `apm` | Local APM pack validation skipped |
| No `pwsh` | PSScriptAnalyzer skipped, even if Windows PowerShell exists |
| `pwsh` present, PSScriptAnalyzer module absent | Failed PowerShell check |

Skipped tools fail strict runs (`--fail-on-skip` or CI). Install only the
prerequisites needed for the chosen validation, reopen the shell if PATH changed,
and rerun the check. Deliberately filtering a task out is not equivalent to
having run it successfully. Canonical commands are in
[Testing](TESTING.md#cli-validation).

## systemd changes are not visible

First distinguish **user scope**, **system scope**, and **no live user manager**.
Bare configured names use user scope. Inspect the correct manager; substitute
the actual configured unit for `example.service`:

```bash
systemctl --user status example.service --no-pager
journalctl --user -u example.service -n 50 --no-pager
```

For a system-scope entry, omit `--user`. Check that required packages, linked
unit files, and executable scripts exist. The task waits for those other tasks
but is not blocked by every unrelated failure they encounter.

In an Arch chroot or without a user manager, dotfiles enables user units
offline; it does not start them. Log in normally before judging runtime state.
For extensions deferred by `install-arch`, inspect:

```bash
journalctl --user -u dotfiles-first-login.service -n 50 --no-pager
```

The first-login work retries until configured extensions converge. In WSL,
systemd must be running or degraded for the unit task to apply at all; restart
the distribution after changing its systemd configuration.
Do not restart the whole desktop as the first diagnostic.

## WSL settings did not change

Run the Linux CLI **inside the distribution**, not the Windows host executable.
Confirm the WSL category selected the `system-files` entry and that it applied
successfully. A dry run does not change `/etc/wsl.conf`.

Changes require a distribution restart. After saving all WSL work, run this
from Windows:

```powershell
wsl --shutdown
```

This terminates **all running WSL distributions**, not just the one being
configured. Reopen the intended distribution afterward.

## Remove did not restore machine defaults

That is expected: remove is not a machine snapshot restore. It materializes
currently selected home links, removes hooks and the launcher still matching
their managed state, and invokes active overlay removal scripts. Modified
hooks/launchers and unrelated hook names are preserved. It leaves packages, registry,
systemd, shell selection, system files, Git/agent settings, PATH, editor
extensions, and APM state.

Use the original profile and overlay so the intended resources are selected.
Do not delete the checkout until materialization succeeds, and remember that
materialized content is the configured source, not your pre-install file.
See [Remove tasks](TASKS.md#remove-tasks).
