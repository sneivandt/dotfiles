# Usage

Use dotfiles to bring a machine into the state described by this checkout.
Start with [Getting started](README.md) for a first installation, or the
[Windows guide](WINDOWS.md) for PowerShell examples.

The commands below assume an installed `dotfiles` launcher and that you are in
the intended repository root. Before the launcher is installed, use
`./bin/dotfiles` on Linux or `.\bin\dotfiles.exe` on Windows. If the binary is
missing, follow [Wrapper bootstrap](#wrapper-bootstrap).

## Everyday commands

| I want to… | Command |
|---|---|
| See the available tasks | `dotfiles list --root . --profile desktop` |
| Preview this checkout | `dotfiles update --root . --profile desktop --no-repo-update --dry-run --verbose` |
| Apply this checkout | `dotfiles update --root . --profile desktop --no-repo-update` |
| Sync the repository and apply updates | `dotfiles update --root . --profile desktop` |
| Validate configuration and repository files | `dotfiles check --root . --profile desktop` |
| Read the latest run | `dotfiles log --verbose` |
| Preview removal | `dotfiles remove --root . --profile desktop --dry-run --verbose` |

Replace `desktop` with `base` for a shell-focused machine. Neither profile is a
user-files-only mode. **Back up existing configuration before the first apply:**
installing links can replace regular files and empty directories without a
backup. Changes to linked application settings may take effect immediately.

## Apply configuration

### Update

`update` is the default command: running `dotfiles` by itself applies an update.
Tasks inspect current state and avoid changes that are already satisfied.
An update can nevertheless change more than the files you just edited:

| What can change | Normal behavior | To leave it alone for this run |
|---|---|---|
| CLI binary | Eligible release binaries check for a newer version before update/remove | Set `DOTFILES_SKIP_SELF_UPDATE=1` |
| Main checkout and Git overlay | Fetch and fast-forward to their upstream branches | Pass `--no-repo-update` |
| Agent dependencies and deployments | Advance eligible refs and reconcile agent packages | Pass `--skip apm` |
| Local Arch package pins | Refresh release versions and checksums | Pass `--skip local-package-versions` |

Task filters do not disable startup behavior. `--no-repo-update` is neither an
offline switch nor a self-update switch. Packages and overlay scripts may
still use the network.

**Package installation is not a harmless refresh.** Existing packages are not
selected just to upgrade them, and Windows does not run `winget upgrade --all`.
But installing missing Arch packages uses `pacman -Syu --needed --noconfirm`,
which can upgrade the wider system.

To apply only your symlink changes, first preview the same scope:

```bash
dotfiles update --root . --profile desktop --no-repo-update --only symlinks --dry-run --verbose
```

After reviewing it, remove `--dry-run` to apply. To suppress CLI self-update
too, prefix the command on Linux with `DOTFILES_SKIP_SELF_UPDATE=1`; see
[Windows bootstrap](WINDOWS.md#bootstrap) for the PowerShell equivalent.

Repository synchronization never resets or rebases local work. If it changes
the checkout, dotfiles restarts to load the new configuration before continuing.
The parent keeps the run lock throughout. See
[Repository update fails](TROUBLESHOOTING.md#repository-update-fails) for dirty
or diverged branches.

For focused dependency updates, see [Agent tooling](AGENT-TOOLING.md) and
[Local package recipes](LOCAL-PACKAGES.md).

### Select tasks

Use selectors from `list`, not package names or partial task names:

```bash
dotfiles list --root . --profile desktop
dotfiles update --root . --profile desktop --no-repo-update --only "symlinks,git-hooks" --dry-run
```

- `--only` runs the named tasks. Repeat the flag or separate selectors with commas.
- `--skip` excludes tasks and takes precedence over `--only`.
- Selectors match exactly after case, punctuation, and whitespace normalization.
  Full task labels also work; arbitrary substrings do not.
- Unknown selectors and a filter selecting no tasks are errors.
- `--only packages` means **all active native packages**, not a single package
  and not the AUR package list.

**Dependencies are not automatically included by `--only`.** An omitted
blocking prerequisite produces a warning and is assumed satisfied. Use
`--with-deps` to include both blocking prerequisites and ordering predecessors:

```bash
dotfiles list --root . --profile desktop --graph update --only systemd --with-deps
dotfiles update --root . --profile desktop --no-repo-update --only systemd --with-deps --dry-run
```

Inspect the graph first: this can add packages, AUR setup, symlinks, and file
permissions. `--skip` is applied after expansion, so it can remove a
prerequisite again. `--with-deps` is available only for update and update graphs.

### Unattended runs

Select the repository and profile explicitly, disable prompts, and require a
complete result when automation depends on one:

```bash
dotfiles update --root . --profile base --no-repo-update --non-interactive --fail-on-skip
```

This **applies** configuration. Add `--dry-run` to preview instead.
Non-terminal stdin and JSON Lines output also disable prompts. Any present
`CI` variable—even `CI=false`—disables prompts and enables strict skip handling.
Tasks that are genuinely inapplicable are not failures.

Only one update, remove, or check can run against a repository at a time.
Previews count too; linked worktrees share the same lock. `list` and `log`
remain available while a run is active.

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

For discovery without task probes, use an **existing binary's** `list` command.
It does not execute tasks or scripts, acquire the run lock, create a run log,
or save profile/overlay selections.

## Discover tasks

```bash
dotfiles list --root . --profile desktop
dotfiles list --root . --profile desktop --all
dotfiles list --root . --profile desktop --graph update
dotfiles list --root . --profile desktop --format json
```

The normal listing groups selectors by update, remove, and check. It hides
tasks whose platform or configuration clearly rules them out; `--all` includes
them. A visible task is a possibility, not proof that its tools are installed
or that anything needs changing.

Graph output shows every node, including internal tasks, with selection labels:
`default`, `requested`, `dependency`, `filtered`, or `skipped`.
`requires <-` means the predecessor must succeed; `after <-` means wait for it
without inheriting its failure. Filters annotate the graph rather than hiding
its excluded nodes. Internal tasks cannot be selected directly.

Use `--format plain` for tab-separated records or `--format json` for JSON.
`list` still needs loadable configuration and an existing profile selection;
it never opens the profile prompt. Explicit linked-worktree overlays retain
their separate confirmation requirement.

## Console output

Check the header's profile, platform, overlay, and dry-run indicator first.
Tasks print as they finish, so output order is not configuration order.

| Status | Meaning |
|---|---|
| `✓` / `CHANGED` or `PASSED` | Changes applied, or a check passed |
| `~` / `PLANNED` | A dry run found changes to make |
| `○` / `CURRENT` | Already satisfied; shown with `--verbose` |
| `⊘` / `SKIPPED` | Work was not performed; read the reason |
| `⊘` / `BLOCKED` | A required predecessor did not complete successfully |
| `⊘` / `INTERRUPTED` | Work stopped during cancellation |
| `✗` / `FAILED` | An operation failed |

`--verbose` adds current tasks, decisions, and timings. `--no-symbols` uses the
text labels above; `NO_COLOR` disables color. Inapplicable and internal tasks
do not appear in normal rows or totals, but remain in logs.

Indented actions describe files or packages. The final counts describe
**tasks**, not the number of action lines. Failed, interrupted, blocked, and
skipped work appears before successful counts. **Exit 0 with skipped work is
not necessarily a complete installation.** Use `--fail-on-skip` when it must be.

## Remove

Preview with the original profile and overlay while their sources still exist:

```bash
dotfiles remove --root . --profile desktop --dry-run --verbose
```

Remove `--dry-run` only after reviewing the result.

| Removed integration | Result |
|---|---|
| Managed home links | Replaced with real copies of the configured sources |
| Managed Git hooks and launcher | Removed only if they still match their managed state |
| Active overlay scripts | Their removal actions run when their checks find managed state |

**Remove is not rollback.** The copies contain current configured content,
not your pre-install files. Missing home targets can also be materialized;
existing non-link targets and replaced links are preserved. Keep the checkout
until materialization succeeds.

Packages, registry settings, PATH, services, system files, permissions, login
shell, Git/agent settings, completions, editor extensions, and agent deployments
remain. See [Remove tasks](TASKS.md#remove-tasks) before planning a cleanup.

## Check

`check` validates configuration and sources and runs available repository
analyzers. It does not install or repair configuration and has no `--dry-run`.
It does write a log, acquire the run lock, and resolve/save selections.

```bash
dotfiles check --root . --profile desktop
```

The `config-warnings` task fails on configuration diagnostics, including
warnings. Missing optional tools are visible skips; `--fail-on-skip` and CI
make them fatal. If `pwsh` exists but PSScriptAnalyzer does not, that check fails.

See [Testing](TESTING.md#cli-validation) for scoped checks and prerequisites.
Task filtering cannot bypass malformed configuration loaded at startup.

## Logs

After a failure, use the exact `dotfiles log --id … -v` command printed by the
CLI. IDs stay stable even after later runs; numeric history indexes do not.

```bash
dotfiles log                          # newest run
dotfiles log --verbose                # newest run, with diagnostics
dotfiles log --list                   # history and stable run IDs
dotfiles log --list --format json
dotfiles log 2                        # third-newest run
dotfiles log --task symlinks --verbose # one task in the newest run
dotfiles log --raw                    # original stored records
```

Use the actual history ID with `--id`, optionally adding `--task` and either
`--verbose` or `--raw`. `--id` cannot be combined with an index, `--list`, or
`--command`; verbose and raw are mutually exclusive.

`--command update`, `remove`, or `check` filters history before selecting an
index. Historical `install` and `uninstall` filters remain supported; they
are not current execution commands.

Normal viewing shows actions, outcomes, warnings, context, and failed-command
output. Verbose adds task starts, checks, timings, and debug details.
An unchecked nonzero command exit is not necessarily a task failure: some
tools use exit codes to report state. Successful stdout is usually not saved;
`--raw` cannot recover output that was never retained. Oversized subprocess
output is marked truncated and is not accepted as a complete inventory.

Each restarted or elevated process has a separate log linked to its parent.
`unfinished` means no finish record exists; the process may still be running.
Retention targets the newest 50 logs without deleting active ones. Reading
logs does not start another logged run.

### Log locations

`DOTFILES_LOG_DIR` overrides the location, including with a relative path.
Otherwise:

| Platform | Preferred location | Home-directory fallback |
|---|---|---|
| Linux | `$XDG_STATE_HOME/dotfiles/logs` | `~/.local/state/dotfiles/logs` |
| Windows | `%LOCALAPPDATA%\dotfiles\logs` | `%USERPROFILE%\AppData\Local\dotfiles\logs` |

Empty or relative standard state/home variables are ignored. The final
fallback is `./dotfiles/logs`. An unreadable directory is an error, not empty
history. **Review logs for private paths, configuration, and tool output before
sharing them.**

## Repository and overlay paths

`--root .` selects the checkout you are standing in. Without `--root`, dotfiles
tries `DOTFILES_ROOT`, candidate paths relative to its executable, then the
current directory. Wrappers set `DOTFILES_ROOT` to their own checkout.

`--root` chooses configuration, **not the executable**. An installed launcher
can still obtain a binary from another checkout. Use the intended checkout's
wrapper or built binary when that distinction matters.

Overlay priority is `--overlay`, nonempty `DOTFILES_OVERLAY`, then local Git
config `dotfiles.overlay`. Relative paths are resolved from the invocation
directory. Unlike `--profile`, an explicit `--overlay` is saved for future
engine runs—even during check or dry-run. `list` does not save it.

An explicit overlay whose `.git` is a file needs a separate confirmation.
Without an interactive terminal it is rejected. Prefer a stable primary
checkout for unattended use. See [Overlays](CONFIGURATION.md#overlays) for
merging and script safety.

## Wrapper bootstrap

The repository wrappers obtain the CLI and forward its arguments:

```bash
./dotfiles.sh list --root . --profile base
```

```powershell
.\dotfiles.ps1 list --root . --profile desktop
```

If a usable cached binary is absent, the wrapper downloads a release into
`bin/` and verifies its SHA-256 checksum. It also verifies build provenance
when `gh` is available. Missing `gh` produces a warning; a failed verification
with `gh` present blocks bootstrap. See
[Build provenance verification](SECURITY.md#build-provenance-verification).

**Even wrapper help, list, and dry-run can download files.** Use an existing
binary directly to avoid bootstrap.

The publisher comes from the checkout's GitHub `origin` remote unless
`DOTFILES_RELEASE_REPO=owner/repo` explicitly selects another publisher.
Unsupported/missing remotes fail instead of falling back to this project's
releases. Forks without release assets can build locally:

```bash
./dotfiles.sh --build list --root . --profile base
```

On Windows, use `.\dotfiles.ps1 --build list --root . --profile desktop`.
This requires the [source-build prerequisites](CONTRIBUTING.md), may download
Cargo dependencies, and writes build artifacts. The wrapper uses `dev-opt`
and executes Cargo's reported artifact, including custom target directories;
it does not need to copy it into `bin/`.

Release binaries retain their publishing repository for self-update.
Changing `origin` or the bootstrap override does not redirect an existing
binary's updates. Normal development builds do not self-update.

After the `launcher` and `path` tasks succeed, open a new shell. The installed
launcher in `~/.local/bin` delegates back to this checkout; it is not a
standalone installation. Keep the checkout at a stable path.

## Shell completions

The `completions` task installs Zsh or PowerShell registration. Open a new shell
after installation. Task completion uses the current binary's read-only
discovery and carries forward `--root`, `--profile`, and `--overlay`; it does
not prompt for or save a profile.

For managed shell behavior beyond the CLI, see [Application settings](APPLICATIONS.md).

## Command summary

| Command | Purpose |
|---|---|
| `update` (default) | Apply desired state and advance eligible dependencies |
| `remove` | Detach a limited set of integrations, not restore machine defaults |
| `check` | Validate configuration and repository files |
| `list` | Discover selectors or inspect a dependency graph |
| `log` | Read retained runs |
| `help [command]` | Show command-specific usage |
| `completions <shell>` | Emit shell registration; hidden support command |

## Command options

Options may appear before or after an explicit command. With no command, they
apply to `update`. Use `dotfiles <command> --help` for full syntax, and
`dotfiles --version` or `-V` for the version.

| Option | Commands | Purpose |
|---|---|---|
| `-p`, `--profile`; `--root`; `--overlay` | update, remove, check, list | Select role and configuration sources |
| `-n`, `--dry-run` | update, remove | Preview planned changes |
| `--only`; `--skip` | update, remove, check; list with `--graph` | Include/exclude selectors |
| `--with-deps` | update; list with an update graph | Expand `--only` to predecessors |
| `--no-repo-update` | update | Use current checkout content |
| `-v`, `--verbose` | update, remove, check, log | Show diagnostics |
| `-q`, `--quiet` | update, remove, check | Keep only warnings and incomplete outcomes |
| `--format human\|jsonl` | update, remove, check | Choose execution output |
| `--no-parallel` | update, remove, check | Run selected work sequentially |
| `--non-interactive` | update, remove, check | Disable prompts |
| `--fail-on-skip` | update, remove, check | Fail on unmet applicable work |
| `--no-symbols` | update, remove, check | Use text status labels |
| `--format table\|plain\|json` | list; log with `--list` | Choose discovery/history output |
| `--graph update\|remove\|check` | list | Inspect a command graph |
| `--all` | list without `--graph` | Include clearly inapplicable tasks |
| `--skip-attestation` | update, remove | Bypass self-update provenance, never checksums |

`--skip-attestation` does not affect bootstrap; `DOTFILES_SKIP_ATTESTATION=1`
affects both layers. Neither is a routine workaround for verification errors.
Selecting `repository` with both `--only` and `--no-repo-update` is an error.

## Execution output for scripts

Quiet mode preserves logs and exit policy. Warnings and failed, blocked,
skipped, or interrupted rows remain on stderr. It conflicts with `--verbose`
and an explicit `--format`.

For machine-readable execution, use JSON Lines:

```bash
dotfiles check --root . --profile base --only config-warnings --format jsonl
```

Each stdout line is one object with `schema: 1`, `run_id`, `context`, and
`type`. Record types include `run_start`, `run_scope`, `run_context`,
`task_result`, `task_duration`, `message`, and `run_finish`. Task results carry
IDs, selectors, statuses, reasons, and action counts; finish records include
outcome and exit code. Stored statuses use names such as `ok`, `dry_run`, and
`not_applicable`, rather than the human display labels.

JSON Lines disables prompts. Parse errors and logging warnings can still
appear on stderr; detailed actions and subprocess diagnostics remain in logs.
Group records by run ID: restarted/elevated children have their own IDs and
`parent_run_id` links. Tolerate additional fields/types and missing finish
records after a force quit.

**Closing stdout does not cancel an update.** A closed pipe stops further
stdout output, but managed work continues.

## Interrupting a run

1. Press Ctrl-C once to stop dispatching new work and wait for in-flight work.
2. Press it again to request a force quit. An interactive prompt defaults to no.
3. Confirming, or pressing Ctrl-C while that prompt is open, exits immediately.
   In non-interactive mode the second Ctrl-C force quits without asking.

Neither graceful cancellation nor force quit rolls back completed changes.
Force quit can leave external installers running; inspect them before retrying.

## Exit behavior

| Exit code | Meaning |
|---|---|
| `0` | Succeeded under the selected skip policy |
| `1` | Configuration/runtime failure, task failure, or unmet work under strict policy |
| `2` | Invalid command-line usage |
| `130` | Interrupted; a genuine task failure takes precedence during graceful shutdown |

For recovery, use [Troubleshooting](TROUBLESHOOTING.md).
