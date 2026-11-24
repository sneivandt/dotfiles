---
title: Commands and usage
description: Preview and apply configuration, select tasks, interpret results, and inspect failed runs.
---

# Commands and usage

These commands use your installed launcher and its checkout. For a first
installation, follow [Getting started](getting-started.md) instead. Before
the launcher is installed, you can use the checkout's `./dotfiles.sh` on
Linux or `.\dotfiles.ps1` on Windows.

## Everyday commands

| Goal | What to run |
|---|---|
| See the available tasks | `dotfiles list` |
| Preview an update without applying the planned changes | `dotfiles update --dry-run` |
| Sync the repository and apply updates | `dotfiles update` |
| Validate configuration and repository files | `dotfiles check` |
| Inspect a managed home symlink | `dotfiles explain <path>` |
| Read the latest run | `dotfiles log` |
| Preview removal | `dotfiles remove --dry-run` |

**Running `dotfiles` with no command applies an update.** To see what would
change, use `--dry-run` explicitly. Neither profile limits work to your home
directory. Back up entries that block managed links before moving them
aside; edits to linked settings can take effect immediately.

Without a saved profile, `update`, `remove`, and `check` can prompt in an
interactive terminal. For `list`, `explain`, or an [unattended run](#unattended-runs),
supply a profile if none is selected. `--profile base` and `--profile desktop`
override the role for one run.

## Dry-run scope

`--dry-run` previews planned configuration changes. **It is not a sandbox or a
promise of zero writes.**

| Activity | What a preview still does |
|---|---|
| Wrapper bootstrap | Can download a binary, or build one with `--build`, before the CLI starts |
| CLI release check | Can read caches and query GitHub; does not replace the binary or write release caches |
| Repository task | Can query remote refs, but does not fetch or merge; omit it with `--no-repo-update` |
| Bookkeeping | Writes a run log and acquires the repository lock |
| Selection | Can save an interactive profile choice or an explicit `--overlay` |
| Inspection | Reads host state and invokes external tools; their effects are not sandboxed |
| Overlay scripts | Executes `--check` and, when needed, `--dryrun`; the scripts must honor those modes |

For discovery without task probes, use an existing binary's `list` command.
It does not execute tasks or scripts, acquire the run lock, create a run log,
or save profile/overlay selections.

## Discover tasks

A task is a named group of work, such as installing packages or creating home
symlinks. One task can perform many individual actions.

Use `dotfiles list` to find the selectors available to your profile and platform.
To include inapplicable tasks or inspect selection and dependencies:

```sh
dotfiles list --all
dotfiles list --graph update
```

A listed task may still need tools that are not installed. A graph explains
selection, not what will change. See [Task reference](../reference/task-reference.md) for
each task and [Task discovery output](../reference/cli-reference.md#task-discovery-output)
for graph markers and structured formats.

## Update

`dotfiles update` is the everyday command for bringing a machine in line with
this checkout. It syncs the repository and applies the selected profile's
packages, links, and settings in one run, so you don't have to pull first or
remember which tasks to run afterward. Tasks leave already-correct state alone.

When you want to change just one thing, narrow the run with `--only`. For
example, to preview changes to managed symlinks:

```sh
dotfiles update --only symlinks --dry-run
```

Remove `--dry-run` after review. Filters select tasks, not the CLI release check.

### Update lifecycle

Updating the CLI, syncing configuration, and upgrading applications are
separate actions:

| Stage | What happens |
|---|---|
| Wrapper bootstrap | Downloads a verified CLI if needed, even before help or preview; see [Wrapper bootstrap](../reference/cli-reference.md#wrapper-bootstrap) |
| CLI self-update | An eligible release binary checks for a newer CLI, verifies and installs it, then restarts; this precedes task selection for update and remove |
| Selected tasks | The repository task fetches and fast-forwards the checkout and active Git overlay, then the remaining tasks apply configuration |

Development builds and read-only commands do not self-update. A release-lookup
failure leaves the existing binary usable; download verification is covered in
[Release downloads](security-and-trust.md#release-downloads).

Repository changes trigger a restart with fresh configuration and can affect
live linked application settings immediately. The
[repository task](../reference/dotfiles-repository.md) reports local changes,
missing upstreams, divergence, and remote failures rather than stashing,
resetting, or rebasing. Earlier successful changes remain after a later failure;
follow linked child logs after a restart or elevation.

A preview cannot inspect commits it has not downloaded, and applying inspects
the machine again rather than replaying a saved plan. Use `--no-repo-update`
for both preview and apply when reviewing a fixed checkout.

Package installation does not keep every installed application current; see
[Package management](package-management.md#install-and-upgrade) for upgrades,
[Agent configuration](agent-configuration.md) for agent dependencies, and
[Application settings](application-settings.md#visual-studio-code) for editor
extensions and plugins.

### Control what advances

Ordinary updates need no extra switches. Use these to narrow a run deliberately:

| Control | Effect and limit |
|---|---|
| `DOTFILES_SKIP_SELF_UPDATE=1` | Keep the CLI version; wrapper bootstrap can still download a missing binary |
| `--no-repo-update` | Keep checkout contents fixed; the CLI release check still runs |
| `--only <selectors>` | Select tasks, not startup checks; add `--with-deps` for prerequisites |
| `--skip agent-packages` | Leave agent deployments alone; preferences and the APM executable are separate |
| `--skip custom-package-versions` | Keep custom package pins; recipe preparation can still use them |

There is no blanket offline switch.

### Select tasks

Use exact selectors from the task listing. For example, select `symlinks` and
`git-hooks` together:

```bash
dotfiles list
dotfiles update --only symlinks,git-hooks --dry-run
```

- `--only` runs the named tasks. Repeat the flag or separate selectors with commas.
- `--skip` excludes tasks and takes precedence over `--only`.
- Selectors match exactly after case, punctuation, and whitespace normalization.
  Full task labels also work; arbitrary substrings do not.
- Unknown selectors and a filter selecting no tasks are errors.
- `--only packages` means **all selected pacman or winget packages**, not a
  single package and not the AUR package list.

**Dependencies are not included automatically.** This lets you select just
`systemd` without also asking dotfiles to install packages. If you omit a required task, dotfiles warns
and proceeds as though that prerequisite had succeeded. Use `--with-deps` when
you want prerequisites and other tasks the selection must wait for:

```bash
dotfiles list --graph update --only systemd --with-deps
dotfiles update --only systemd --with-deps --dry-run
```

Inspect the graph first: the systemd example adds package work, AUR setup,
custom package preparation, symlinks, and the dotfiles launcher where applicable.
Other selections, such as `git-hooks`, can also add repository synchronization.
`--skip` is applied after expansion, so it can remove a prerequisite again.
`--with-deps` is available only for update and update graphs.

### Unattended runs

Select the profile explicitly, disable prompts, and require a
complete result when automation depends on one:

```bash
dotfiles update --profile base --non-interactive --fail-on-skip
```

This **applies** configuration. Add `--dry-run` to preview instead.
Non-terminal stdin and JSON Lines output also disable prompts. Any present
`CI` variable (even `CI=false`) disables prompts and enables strict skip handling.
Tasks that do not apply to the selected platform or configuration are not failures.

Only one update, remove, or check can run against a repository at a time.
Previews count too; linked worktrees share the same lock. `list` and `log`
remain available while a run is active.

## Console output

Read the header first to confirm the profile, platform, overlay, and run mode.
A CLI upgrade or repository sync appears beneath it; when a sync restarts the
process to load new configuration, task output continues without a second header.
In an interactive terminal, a spinner shows in-flight work, and tasks print as
they finish rather than in configuration order. Read the status and indented
actions together: one task can change many files or packages.

| Status | Meaning |
|---|---|
| `✓` / `CHANGED` or `PASSED` | Changes applied, or a check passed |
| `~` / `PLANNED` | A dry run found changes to make |
| `○` / `UNCHANGED` | Already matches configuration; shown with `--verbose` |
| `⊘` / `SKIPPED` | Work was not performed; read the reason |
| `⊘` / `BLOCKED` | A required predecessor did not complete successfully |
| `⊘` / `INTERRUPTED` | Work stopped during cancellation |
| `✗` / `FAILED` | An operation failed |

`--verbose` adds current tasks, decisions, and timings. `--no-symbols` uses the
text labels above and an ASCII spinner. A nonempty `NO_COLOR` disables styling,
including profile-prompt emphasis, but not functional terminal redraws.
Redirected streams have no ANSI styling. Inapplicable and internal tasks
do not appear in normal rows or totals, but remain in logs.

Task warnings appear alongside the actions they concern. Even in quiet mode,
a warning from an unchanged task still appears on stderr with the task name.

The final summary names its unit once, as in **Tasks · 3 changed · 14 unchanged**
or **Checks: 5 passed · 1 skipped**. Previews use **planned** instead of **changed**.
These are task counts, not action counts or CLI upgrades. **Unchanged** means
the selected work needed no changes, not that every installed package is at its
latest version. Repository syncs that restart the command appear in their notice
rather than the child's task totals. When no selected tasks apply, the summary
says **No applicable tasks**.

An illustrative update that upgraded the CLI and synced the repository, but
needed no further task changes:

```text
Update · desktop profile · Arch Linux

CLI upgraded · v2026.01.01-1 → v2026.01.01-2
Repository synced · restarting to load configuration

Tasks · 17 unchanged · 3.4s
```

The final duration covers the CLI invocation, including self-update and
repository restarts; retained logs keep separate timings for each process.
Failed, interrupted, blocked, and skipped work appears before successful counts.
Failure and interruption remain explicit even when the affected work is
internal and absent from visible task counts.
**Exit 0 with skipped work is not necessarily a complete installation.**
Use `--fail-on-skip` when it must be.

After a failure or force quit, copy the log command if one is printed. Its
`--id` selects that exact run even after another invocation, and `-v` shows
the diagnostics. If logging failed, keep the terminal error instead.

## Check configuration

The check command validates configuration, checks source files, and runs
available code analyzers. It does not install or repair configuration and has
no `--dry-run`. It writes a log and holds the run lock. It can save a profile
chosen at the prompt or an overlay passed with `--overlay`.

```bash
dotfiles check
```

The `config-diagnostics` task fails on configuration diagnostics, including
warnings. Missing optional tools are visible skips; `--fail-on-skip` and CI
make them fatal.

See [Testing](../development/testing.md#cli-validation) for scoped checks and prerequisites.
Task filtering cannot bypass malformed configuration loaded at startup.

## Logs

After a failure, use the log command printed by the CLI. Its `--id` selects
that exact run even after later invocations.

```sh
dotfiles log                          # newest run
dotfiles log --list                   # history and stable run IDs
dotfiles log --id <run-id> --task symlinks --verbose
```

Replace `<run-id>` and the task selector (`symlinks`) with the values from the
failed run. If the process restarted or delegated elevation, follow the linked
child log. Review logs for private paths, configuration, and tool output before
sharing them; see the [log reference](../reference/cli-reference.md#logs) for
filters, raw records, retention, and storage locations.

## Inspect a home path

Use `dotfiles explain <path>` to see which declaration manages a home symlink.
Pass a path relative to your home directory or an absolute path. For example,
to inspect your Bash startup file:

```bash
dotfiles explain .bashrc
```

On Linux, `~/.bashrc` also works. The output shows the declaration, source,
resolved target, and what update and remove would do. Use the same profile and
overlay you use for updates. An existing CLI binary inspects the path without
starting a task or writing a run log. If a custom `XDG_CONFIG_HOME` blocks
symlink installation, `explain` reports that under **Update** and still shows
what **Remove** would do at the configured home path.

## Remove integrations

Preview with the original profile and overlay while their sources still exist:

```bash
dotfiles remove --dry-run
```

Remove `--dry-run` only after reviewing the result. If your saved selection has
changed, pass the original `--profile` and `--overlay` explicitly.

| Removed integration | Result |
|---|---|
| Managed home links | Replaced with real copies of the configured sources |
| Managed Git hooks and launcher | Removed only if they still match their managed state |
| Active overlay scripts | Their removal actions run when their checks find managed state |

Home-link actions read `copy <home path> from <source>`. **Remove is not
rollback:** copies contain current configured content, not your pre-install
files. Existing ordinary files, unrelated links, and missing targets are left
alone (add `--restore-missing` with `--only symlinks` to also copy at absent
targets). Keep the checkout until copying succeeds. See
[Home symlinks removal](../reference/home-symlinks.md#removal) for metadata,
permission, and nested-link copy rules.

Packages, registry settings, PATH, services, system files, permissions, login
shell, Git/agent settings, completions, editor extensions, and agent deployments
remain. See [Remove tasks](../reference/task-reference.md#remove-tasks) before planning a cleanup.

## Move a checkout

Keep the checkout at a permanent location when possible. Moving it does not
retarget installed home links or the launcher, and `remove` is not a relocation
command. These steps assume an already configured machine with its existing
tools, profile, and overlay still available.

1. Stop dotfiles runs and save work in applications using linked settings.
   Record the active profile and overlay, preserve local edits, and keep a
   recoverable copy of the sources. Move an ordinary checkout with your normal
   file tools; use Git's worktree relocation procedure for a linked worktree.
2. Open a terminal in the new checkout. Correct any `DOTFILES_ROOT` override
   pointing to the old directory. If the overlay also moved, update its
   selection and any `DOTFILES_OVERLAY` override. Use this checkout's wrapper
   rather than the old launcher.
3. Preview the path-dependent integrations using the same profile and overlay
   as before. On Linux:

   ```sh
   ./dotfiles.sh update --no-repo-update --only launcher,symlinks,git-hooks,completions --dry-run
   ```

   On Windows:

   ```powershell
   .\dotfiles.ps1 update --no-repo-update --only launcher,symlinks,git-hooks,completions --dry-run
   ```

4. Inspect each reported conflict before changing it. The symlinks task
   preserves links to old sources rather than retargeting them automatically.
   Use `readlink` on Linux or `Get-Item` with `LinkType` and `Target` on Windows
   to confirm the old destination, then move only that obsolete link itself
   aside. Do not move or delete the directory contents reached through a link
   or junction. Preserve unrelated files and modified hooks or launchers.
5. Repeat the preview until it proposes the intended new paths without unresolved
   conflicts, then remove `--dry-run` to apply the reviewed work. Keep `--build`
   after the wrapper name if you use source builds. Open a new shell and repeat
   the [installation checks](getting-started.md#5-verify-the-installation).

Do not delete the old sources or backups until the selected links, launcher,
and affected applications work from the new location. Custom scripts and
application settings can contain additional absolute paths that this scoped
update does not rewrite.

## Shell completions

The `completions` task installs Zsh or PowerShell registration. Open a new shell
after installation. Task completion uses the current binary's read-only
discovery and carries forward `--root`, `--profile`, and `--overlay`; it does
not prompt for or save a profile.

For managed shell behavior beyond the CLI, see [Application settings](application-settings.md).

## Interrupting a run

1. Press Ctrl-C once to stop dispatching new work and wait for in-flight work.
2. Press it again to request a force quit. An interactive prompt defaults to no.
3. Confirming, or pressing Ctrl-C while that prompt is open, exits immediately.
   In non-interactive mode the second Ctrl-C force quits without asking.

Neither graceful cancellation nor force quit rolls back completed changes.
Force quit can leave external installers running; inspect them before retrying.

For exhaustive options and exit codes, use the [CLI reference](../reference/cli-reference.md).
