# Usage

The project is operated by a Rust CLI named `dotfiles`. The repository-level
wrappers download or build that CLI, then forward arguments without
reimplementing its behavior.

## Bootstrap

### Linux

```bash
./dotfiles.sh install --profile base --dry-run
./dotfiles.sh install --profile base
```

### Windows

```powershell
.\dotfiles.ps1 install --profile desktop --dry-run
.\dotfiles.ps1 install --profile desktop
```

By default, a wrapper uses `bin/dotfiles` or `bin\dotfiles.exe`. If the binary
is absent, it downloads the latest compatible GitHub Release asset, verifies its
SHA-256 checksum, and verifies its build provenance attestation when the `gh`
CLI is available. Use wrapper-only `--build` to compile the CLI with Cargo:

```bash
./dotfiles.sh --build install --dry-run
```

```powershell
.\dotfiles.ps1 --build test
```

After installation, `~/.local/bin/dotfiles` is the normal entry point.

## Command summary

| Command | Behavior |
|---|---|
| `install` | Converges the configured machine state without advancing pinned dependency versions |
| `update` | Runs the install graph and includes version-advancing update tasks |
| `uninstall` | Removes managed integrations while preserving user files and broader machine state |
| `test` | Validates configuration and runs available script analyzers |
| `tasks` | Lists visible task selectors, labels, and command membership |
| `log` | Lists retained run logs or prints one of them |
| `completions <shell>` | Hidden support command that emits shell completion definitions |

## Global options

Global options may be placed before or after the subcommand.

| Option | Meaning |
|---|---|
| `-v`, `--verbose` | Show additional diagnostic task output, and diagnostic lines in `dotfiles log` |
| `-p`, `--profile <PROFILE>` | Select a role profile for this run |
| `-d`, `--dry-run` | Plan and report changes without applying them |
| `--root <PATH>` | Treat another path as the dotfiles repository |
| `--overlay <PATH>` | Append configuration from a private overlay repository |
| `--no-parallel` | Run independent tasks sequentially |
| `--version` | Print the CLI version |

`--dry-run` applies to mutating commands. It is the recommended first step
after changing profiles or configuration.

## Install

```bash
dotfiles install
dotfiles install --profile desktop
dotfiles install --dry-run --verbose
```

`install` is idempotent: each task inspects current state and only applies the
required change. Independent ready tasks may run concurrently; explicit
dependencies preserve ordering.

### Select tasks

`--only` and `--skip` accept comma-separated, case-insensitive task selectors.
Punctuation and whitespace are normalized to hyphens. Each value must exactly
match either a task's stable selector or its full normalized display label.
Use `dotfiles tasks` to discover the supported selectors.

```bash
dotfiles install --only symlinks
dotfiles install --only "packages,git-hooks"
dotfiles install --skip "systemd,registry"
```

Both selectors can be used together: `--only` first limits the candidate set,
then `--skip` removes matches. Matching is not based on Rust type names,
arbitrary substrings, action-prefix removal, or the first word of a label.
For example, `repository` and `dotfiles-repository` both match **Dotfiles
repository**, but `dotfiles` does not. Unmatched selectors produce a warning.
Internal orchestration tasks are omitted from discovery and cannot be selected.

## Discover tasks

```bash
dotfiles tasks
```

The output contains `SELECTOR`, `TASK`, and `COMMANDS` columns. It combines
install, update, uninstall, test, and active overlay-script tasks, while hiding
internal orchestration. Rows retain catalog/discovery order; the command does
not sort them. A selector is rejected if it maps to conflicting display labels.

## Update

```bash
dotfiles update
dotfiles update --only apm,apm-update
```

`update` uses the same dependency scheduler and selectors as `install`. The
difference is that it includes update-only tasks, which may advance pinned
dependency versions. Normal repeatable convergence should use `install`.

Repository synchronization occurs during both commands when the checkout can be
updated. If the repository changes, the CLI reloads configuration before
downstream tasks consume it.

## Console output

Every run opens with a dimmed header line that names the command, resolved
profile, and platform. Two optional sections may follow, in this order:
`dry run` for a dry run, and `overlay <path>` when an overlay is active.

```text
Install · profile desktop · Arch Linux · dry run · overlay ~/src/dotfiles-private
```

Visible task rows are printed as tasks complete, so independent parallel tasks
may appear in a different order between runs. Statuses distinguish the outcome:

| Status | Meaning |
|---|---|
| `CHANGE` | The task applied one or more changes |
| `DRYRUN` | Dry-run changes were planned but not applied |
| `PASSED` | A validation task passed |
| `IGNORE` | The task was intentionally ignored |
| `FAILED` | The task failed |
| `OK` | The task was already up to date (verbose only) |
| `N/A` | The task does not apply to this platform or configuration (verbose only) |

A row states the task name and, when the task has something to explain, the
reason after a `·` separator:

```text
IGNORE Dotfiles repository · local changes present
```

Indented dimmed lines beneath a row are the individual actions the task took or
planned, listed in full without truncation. A blank line separates a task that
printed actions from the row that follows it.

Normal output only prints rows for tasks that did something or need attention.
`--verbose` accounts for every task — including up-to-date and non-applicable
ones — adds elapsed time to each row, and replays the per-resource decisions
behind each outcome. Internal orchestration remains available in the run log but
is excluded from console rows and totals in both modes.

While tasks are running, a transient status line reports progress and the
currently active tasks:

```text
Running 12/16 · Home symlinks, System packages
```

Its denominator counts the same tasks the summary accounts for. Applicability is
only known once a task has run, so a task that turns out not to apply leaves the
denominator rather than advancing the numerator.

The final line reports actual action counts and affected task counts, for
example
`Applied 4 changes across 2 tasks · 14 up to date · 1 ignored · 2.3s`,
`Dry run · 121 changes across 5 tasks · 8 up to date · 3 ignored · 0.7s`, or
`6 passed · 1 ignored · 1.4s`.

## Uninstall

```bash
dotfiles uninstall --dry-run
dotfiles uninstall
```

Uninstall is intentionally conservative. It:

1. Replaces managed home-directory symlinks with materialized files or
   directories.
2. Removes installed repository Git hooks.
3. Removes the installed CLI wrapper.

It does **not** uninstall packages, revert registry values, disable systemd
units, undo shell selection, or reverse arbitrary overlay scripts. See
[Uninstall tasks](TASKS.md#uninstall-tasks).

## Test

```bash
dotfiles test
dotfiles test --verbose
dotfiles test --overlay C:\path\to\private-dotfiles
```

The command validates TOML, sources, manifest section synchronization, and APM
plugin references. ShellCheck and APM checks are skipped when their executables
are unavailable. The PowerShell check runs whenever `pwsh` is available; if the
PSScriptAnalyzer module is missing, that check fails and reports the PowerShell
error.

## Logs

Every run writes its own log file. The most recent 50 are retained, so a failed
run is still readable after later runs have happened.

```bash
dotfiles log            # newest run
dotfiles log --list     # retained runs, newest first
dotfiles log 2          # third-newest run
dotfiles log -c install # newest install run
dotfiles log --verbose  # include diagnostic lines
```

`--list` prints an index, timestamp, command, and size:

```text
  #  WHEN                  COMMAND    SIZE
  0  2026-07-31 15:42:10Z  install  112.4 KB
  1  2026-07-31 15:39:02Z  test       8.1 KB
```

The index argument selects from that list and is stable for the duration of a
listing; it shifts as new runs are recorded. `--command` filters the list and
renumbers it, so `dotfiles log -c install 1` means the second-newest install.

Logs contain every event in the order it actually happened, including messages
the console suppresses. Lines are
`seq | elapsed_us | wall_utc | context | event | message`, where `context` is the
task that produced the event, so parallel execution can be reconstructed. Each
executed task also emits a `task_timing` event recording how long it ran, which
is otherwise not derivable from a parallel run's interleaved timestamps.

Without `--verbose`, `dotfiles log` hides `debug` events, matching what the
console shows during a normal run. `--verbose` prints the file unfiltered.

Logs live in a platform state directory, resolved in this order:

| Order | Location |
|---|---|
| 1 | `$DOTFILES_LOG_DIR` when set, used as-is |
| 2 | `%LOCALAPPDATA%\dotfiles\logs` |
| 3 | `$XDG_STATE_HOME/dotfiles/logs` |
| 4 | `~/.local/state/dotfiles/logs` |
| 5 | `./dotfiles/logs` |

Files are named `<utc-timestamp>-<command>-<pid>.log`. Logs written by earlier
versions under the cache directory are removed on the next run.

## Repository and overlay paths

The wrappers set the repository root automatically. Direct CLI usage normally
discovers it from the installed wrapper environment; use `--root` when running
against a different checkout:

```bash
dotfiles --root C:\Code\sneivandt\dotfiles test
```

An overlay is an additional repository whose matching configuration is appended
to the main configuration:

```bash
dotfiles install --overlay C:\Code\private-dotfiles
```

Only overlay repositories can define `conf/scripts.toml`. See
[Configuration overlays](CONFIGURATION.md#overlays).

## Exit behavior

A command exits unsuccessfully when required configuration cannot be loaded, a
task fails, or validation reports an error. Non-applicable tasks and optional
tool checks are recorded separately from failures. Use `--verbose` and
`dotfiles log --verbose` when diagnosing a failed run.
