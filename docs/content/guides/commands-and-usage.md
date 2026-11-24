---
title: Commands and usage
description: Preview and apply configuration, select tasks, interpret results, and inspect failed runs.
---

# Commands and usage

Once your machine is set up, `dotfiles` is your day-to-day entrypoint for
previewing, applying, validating, and inspecting configuration. If you haven't
installed the launcher yet, start with [Getting started](getting-started.md),
or invoke the checkout's `./dotfiles.sh` on Linux or `.\dotfiles.ps1` on
Windows directly.

## Everyday commands

| Goal | What to run |
|---|---|
| See the available tasks | `dotfiles list` |
| Preview an update without applying changes | `dotfiles update --dry-run` |
| Sync the repository and apply updates | `dotfiles update` |
| Validate configuration and repository files | `dotfiles check` |
| Inspect a managed home symlink | `dotfiles explain <path>` |
| Read the latest run log | `dotfiles log` |
| Preview removal | `dotfiles remove --dry-run` |

**Running `dotfiles` with no subcommand applies an update.** Always pass
`--dry-run` when you only want to inspect what would change. Keep in mind that
neither `base` nor `desktop` limits changes to your home directory, that
existing files blocking managed links should be backed up before moving them
aside, and that edits to linked settings can take effect immediately.

If no profile is saved yet, `update`, `remove`, and `check` prompt in an
interactive terminal. For `list`, `explain`, or any
[unattended run](#unattended-runs), pass `--profile base` or `--profile desktop`
explicitly. Explicit `--profile` flags override the saved role for that single
run.

## Dry-run scope

`--dry-run` previews planned configuration changes without applying them, but
**it is not a sandbox or a guarantee of zero side effects**:

| Activity | What a preview still does |
|---|---|
| Wrapper bootstrap | Can download a CLI binary, or build one with `--build`, before the CLI starts |
| CLI release check | Can read local caches and query GitHub; does not replace the binary or write release caches |
| Repository task | Can query remote refs without fetching or merging; omit it with `--no-repo-update` |
| Bookkeeping | Writes a run log and acquires the repository lock |
| Selection | Can save an interactive profile choice or an explicit `--overlay` |
| Inspection | Reads host state and invokes external inspection tools outside a sandbox |
| Overlay scripts | Runs `--check` and, when work is needed, `--dryrun`; the scripts themselves must stay read-only |

If you want to inspect available tasks without probing the machine, run `list`
using an existing binary. `dotfiles list` skips task probes, overlay scripts,
the run lock, run logs, and saving profile or overlay selections.

## Discover tasks

A task is a named unit of work, such as installing packages or managing home
symlinks, and a single task can perform many individual actions.

Run `dotfiles list` to see the task selectors active for your profile and
platform. To include inapplicable tasks or inspect dependencies:

```sh
dotfiles list --all
dotfiles list --graph update
```

A listed task may still depend on tools that are not installed yet, and a graph
explains scheduling relationships rather than predicting what will change. See
[Task reference](../reference/task-reference.md) for what each task owns and
[Task discovery output](../reference/cli-reference.md#task-discovery-output)
for graph markers and structured output formats.

## Update

`dotfiles update` brings your machine in line with the checkout in a single
run. It syncs the repository and applies the selected profile's packages,
links, and settings, leaving already-correct state alone so you don't have to
pull first or remember individual tasks afterward.

When you only want to touch one area, narrow the run with `--only`. For
example, to preview changes to managed symlinks:

```sh
dotfiles update --only symlinks --dry-run
```

Remove `--dry-run` after reviewing the output. Task filters narrow which tasks
run, not the startup CLI release check.

### Update lifecycle

Updating the CLI binary, syncing configuration, and upgrading installed
applications happen at different stages:

| Stage | What happens |
|---|---|
| Wrapper bootstrap | Downloads a verified CLI if needed, even before help or a preview; see [Wrapper bootstrap](../reference/cli-reference.md#wrapper-bootstrap) |
| CLI self-update | An eligible release binary checks for a newer CLI, verifies and installs it, then restarts before task selection for `update` and `remove` |
| Selected tasks | The repository task fetches and fast-forwards the checkout and any active Git overlay, then the remaining tasks apply configuration |

Development builds and read-only commands never self-update. If a release
lookup fails, the existing binary continues normally; download verification is
explained in [Release downloads](security-and-trust.md#release-downloads).

When repository sync pulls new commits, the CLI restarts to reload the updated
configuration before running later tasks, and live linked application settings
can change right away. The [repository task](../reference/dotfiles-repository.md)
reports local edits, missing upstreams, diverged branches, and remote errors
instead of stashing, resetting, or rebasing. If a later task fails, earlier
completed changes stay in place; follow linked child logs whenever a run
restarts or delegates elevation.

Because a preview cannot inspect commits it has not fetched, and because an
actual run re-inspects the machine rather than replaying a saved plan, pass
`--no-repo-update` to both preview and apply when you want to review a fixed
checkout.

Finally, package installation ensures declared packages are present rather than
keeping every installed application up to date. See
[Package management](package-management.md#install-and-upgrade) for upgrades,
[Agent configuration](agent-configuration.md) for agent dependencies, and
[Application settings](application-settings.md#visual-studio-code) for editor
extensions and plugins.

### Control what advances

Routine updates rarely need extra flags, but you can pin specific stages when
needed:

| Control | Effect and limit |
|---|---|
| `DOTFILES_SKIP_SELF_UPDATE=1` | Keeps the current CLI version; wrapper bootstrap can still download a missing binary |
| `--no-repo-update` | Keeps checkout contents fixed; the CLI release check still runs |
| `--only <selectors>` | Runs only the named tasks without disabling startup checks; add `--with-deps` for prerequisites |
| `--skip agent-packages` | Leaves agent deployments alone; settings fragments and the APM binary are separate |
| `--skip custom-package-versions` | Keeps existing custom package pins; recipe preparation can still use them |

There is no single blanket offline switch.

### Select tasks

Use exact selectors from `dotfiles list`. For example, to preview `symlinks`
and `git-hooks` together:

```bash
dotfiles list
dotfiles update --only symlinks,git-hooks --dry-run
```

- `--only` runs the named tasks; repeat the flag or separate selectors with commas.
- `--skip` excludes tasks and takes precedence over `--only`.
- Selectors match exactly after normalizing case, punctuation, and whitespace.
  Full task labels also work, but partial substrings do not.
- Unknown selectors and filters that match zero tasks are errors.
- `--only packages` covers **all selected pacman or winget packages**, not a
  single package and not the AUR package list.

**Dependencies are not pulled in automatically.** That makes it easy to run
just `systemd` without also asking dotfiles to install packages. When you omit
a required prerequisite, dotfiles warns and continues as though it had
succeeded. Add `--with-deps` when you do want prerequisites and ordering
dependencies included:

```bash
dotfiles list --graph update --only systemd --with-deps
dotfiles update --only systemd --with-deps --dry-run
```

Check the graph first so you know what `--with-deps` adds: for `systemd`, it
can pull in package installation, AUR setup, custom package preparation,
symlinks, and the launcher. Other selections, such as `git-hooks`, can add
repository sync. `--skip` is evaluated after expansion so you can still exclude
specific dependencies. `--with-deps` applies only to `update` and update graphs.

### Unattended runs

In scripts or CI, specify the profile explicitly, disable interactive prompts,
and fail on skipped work if a complete run is required:

```bash
dotfiles update --profile base --non-interactive --fail-on-skip
```

This command **applies** configuration; add `--dry-run` to preview instead.
Redirected stdin and JSON Lines output also disable prompts automatically, and
any `CI` environment variable (even `CI=false`) disables prompts while enabling
`--fail-on-skip`. Tasks that simply do not apply to the current platform or
configuration never count as failures.

Only one `update`, `remove`, or `check` command can run against a repository at
a time (including previews and linked worktrees sharing the repository lock).
`list` and `log` remain usable while another run is active.

## Console output

Start with the header line to verify the profile, platform, overlay, and run
mode. Any CLI upgrade or repository sync notice appears right beneath it; when
a sync restarts the process to reload configuration, task output continues
cleanly without printing a second header. In an interactive terminal, a spinner
tracks active work while tasks print as they finish rather than in file order.
Read each status line together with its indented actions, since one task may
modify many files or packages.

| Status | Meaning |
|---|---|
| `✓` / `CHANGED` or `PASSED` | Changes were applied, or a check passed |
| `~` / `PLANNED` | A dry run found changes to make |
| `○` / `UNCHANGED` | Already matches configuration; shown with `--verbose` |
| `⊘` / `SKIPPED` | Work was not performed; check the printed reason |
| `⊖` / `BLOCKED` | A required predecessor did not succeed |
| `⊗` / `INTERRUPTED` | Work stopped during cancellation |
| `✗` / `FAILED` | An operation failed |

Pass `--verbose` to include unchanged tasks, decisions, and timings, or
`--no-symbols` for plain text labels and an ASCII spinner. A nonempty
`NO_COLOR` turns off ANSI styling (including profile-prompt highlighting) while
keeping terminal redraws working; redirected streams omit ANSI styling
automatically. Inapplicable and internal tasks stay out of normal rows and
totals, though they are still recorded in logs.

Task warnings print beside the actions they affect. Even in `--quiet` mode, a
warning from an unchanged task still prints to stderr along with the task name.

The final summary reports task counts (not individual actions or CLI upgrades),
such as **Tasks · 3 changed · 14 unchanged** or **Checks · 5 passed · 1
skipped**, with previews using **planned** in place of **changed**.
**Unchanged** means every selected task already matched its declaration, not
that all installed software is on the latest upstream version. Repository syncs
that trigger a restart appear in their notice rather than the child process's
task totals. When nothing applies, the summary prints **No applicable tasks**.

Here is an illustrative update that upgraded the CLI and synced the repository,
with all configured tasks already up to date:

```text
Update · desktop profile · Arch Linux

CLI upgraded · v2026.01.01-1 → v2026.01.01-2
Repository synced · restarting to load configuration

Tasks · 17 unchanged · 3.4s
```

The reported duration spans the whole CLI invocation, including self-update and
repository restarts, while saved logs keep separate timings per process.
Failed, interrupted, blocked, and skipped counts appear ahead of successful
ones, and failures or interruptions are always reported even when they happen
in internal tasks outside the visible count. **An exit code of 0 with skipped
tasks is not necessarily a complete installation**; pass `--fail-on-skip` when
every applicable task must succeed.

Whenever a run fails or is force-quit, copy the suggested `dotfiles log`
command if one is printed. Its `--id` pins that exact run even after later
invocations, and `-v` reveals full diagnostics. If logging itself failed, keep
the terminal output instead.

## Check configuration

`dotfiles check` validates your declarations, verifies tracked source files,
and runs available static analyzers without changing machine state. It has no
`--dry-run` flag, writes a run log, holds the repository lock, and can still
save an interactively chosen profile or explicit `--overlay`.

```bash
dotfiles check
```

The `config-diagnostics` task fails on every configuration diagnostic,
including warnings. Missing optional analyzers show up as skips unless you pass
`--fail-on-skip` or run under `CI`. Note that `--only` cannot bypass syntax or
schema errors in configuration files loaded at startup.

See [Testing](../development/testing.md#cli-validation) for scoped validation
commands and analyzer prerequisites.

## Logs

When something goes wrong, run the log command printed at the end of the failed
run, or browse recent history directly:

```sh
dotfiles log                          # newest run
dotfiles log --list                   # history and stable run IDs
dotfiles log --id <run-id> --task symlinks --verbose
```

Substitute the run ID and task selector (such as `symlinks`) from your run. If
the CLI restarted or delegated elevation, follow the linked child log ID shown
in the output. Always review logs for private paths, settings, or tool output
before sharing them. See the [log reference](../reference/cli-reference.md#logs)
for filters, raw JSONL records, retention rules, and storage paths.

## Inspect a home path

Use `dotfiles explain <path>` to see which declaration owns a home symlink and
what `update` and `remove` would do for it. Pass either a path relative to your
home directory or an absolute path:

```bash
dotfiles explain .bashrc
```

On Linux, `~/.bashrc` works too. Make sure you use the same profile and overlay
you use for updates. Using an existing CLI binary, `explain` is read-only: it
starts no tasks and writes no run log. If a custom `XDG_CONFIG_HOME` blocks
symlink installation, `explain` reports that under **Update** while still
showing what **Remove** would do at the declared home path.

## Remove integrations

Always preview removal using the same profile and overlay that originally
managed the integrations, while the source checkout is still available:

```bash
dotfiles remove --dry-run
```

Drop `--dry-run` only after checking the plan. If your saved selection has
changed since installation, pass the original `--profile` and `--overlay`
explicitly.

| Removed integration | Result |
|---|---|
| Managed home links | Replaced with standalone copies of the current source files |
| Managed Git hooks and launcher | Deleted only if they still match their managed contents |
| Active overlay scripts | Runs their `--remove` action when `--check` (or `--check-remove`) finds managed state |

Home-link removals print as `copy <home path> from <source>`. **Remove is not
a rollback:** the copies contain the checkout's current contents, not whatever
existed before you installed dotfiles. Ordinary files, unrelated links, and
missing targets are untouched (pass `--restore-missing` with `--only symlinks`
if you also want copies created at missing targets). Keep the checkout intact
until removal succeeds; see
[Home symlinks removal](../reference/home-symlinks.md#removal) for metadata,
permission, and nested-link rules.

Packages, registry values, PATH entries, services, system files, file
permissions, login shell, Git and agent settings, completions, editor
extensions, and agent deployments stay in place. Review
[Remove tasks](../reference/task-reference.md#remove-tasks) before planning a
broader cleanup.

## Move a checkout

Keep your checkout in a permanent directory whenever possible. Moving the
folder does not automatically retarget existing home symlinks or the launcher,
and `dotfiles remove` is not a relocation tool. The steps below assume an
already-configured machine with its tools, profile, and overlay still intact:

1. Stop any running dotfiles commands and save work in applications that read
   linked configuration. Note your active profile and overlay, keep any local
   edits safe, and keep a recoverable copy of your sources. Move an ordinary
   checkout with your regular file tools, or use Git's worktree relocation flow
   for a linked worktree.
2. Open a terminal in the new checkout path. Update any `DOTFILES_ROOT`
   environment override still pointing to the old location (and update your
   overlay selection or `DOTFILES_OVERLAY` if the overlay moved too). Use the
   new checkout's wrapper rather than the old launcher.
3. Preview the path-dependent tasks with the same profile and overlay you used
   previously. On Linux:

   ```sh
   ./dotfiles.sh update --no-repo-update --only launcher,symlinks,git-hooks,completions --dry-run
   ```

   On Windows:

   ```powershell
   .\dotfiles.ps1 update --no-repo-update --only launcher,symlinks,git-hooks,completions --dry-run
   ```

4. Review each reported conflict before touching it. The symlinks task leaves
   links pointing to old locations alone rather than overwriting them. Confirm
   each old target with `readlink` on Linux (or `Get-Item` with `LinkType` and
   `Target` on Windows), then move only the obsolete symlink itself aside.
   Never move or delete the underlying directory contents reached through a
   link or junction, and preserve unrelated files or modified hooks and
   launchers.
5. Re-run the preview until it proposes the new paths cleanly with no
   unresolved conflicts, then run the same command without `--dry-run` (keep
   `--build` right after the wrapper name if you build from source). Open a new
   shell and repeat the
   [verification checks](getting-started.md#5-verify-the-installation).

Keep the old sources and backups until the relocated links, launcher, and
affected applications are all working cleanly. Custom scripts and application
settings may still contain hardcoded absolute paths that this scoped update
does not rewrite.

## Shell completions

The `completions` task installs tab completion for Zsh or PowerShell. Open a
new shell after it runs. Completion queries the current binary's read-only task
discovery and carries forward `--root`, `--profile`, and `--overlay` without
prompting for or saving a profile.

For managed shell startup files, prompts, and aliases beyond the CLI itself,
see [Application settings](application-settings.md).

## Interrupting a run

1. Press Ctrl-C once to stop scheduling new tasks while letting in-flight work
   finish cleanly.
2. Press Ctrl-C a second time to request a force quit. In an interactive
   terminal, the confirmation prompt defaults to no.
3. Confirming (or pressing Ctrl-C again while the prompt is open) exits
   immediately. In non-interactive mode, the second Ctrl-C force-quits without
   prompting.

Neither graceful cancellation nor force quit rolls back changes that already
finished. A force quit can also leave external installers running in the
background, so check them before retrying.

Next, see [Profiles and categories](profiles-and-categories.md) to understand
how roles and operating systems decide what runs, or consult the
[CLI reference](../reference/cli-reference.md) for the complete list of flags,
environment variables, and exit codes.
