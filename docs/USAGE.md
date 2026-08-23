# Usage

The `dotfiles` Rust CLI manages the repository's configured state. The
repository wrappers download or build the CLI, then forward arguments to it.

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

A wrapper uses `bin/dotfiles` or `bin\dotfiles.exe`. If that binary is absent,
the wrapper downloads the latest compatible GitHub Release asset and verifies
its SHA-256 checksum. It verifies build provenance when `gh` is available and
warns without blocking initial bootstrap when `gh` is absent.
`DOTFILES_SKIP_ATTESTATION=1` explicitly bypasses the check even when `gh` is
installed. Use the wrapper-only `--build` option to compile with Cargo:

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
| `--offline` | Converge the current checkout without synchronizing the repository |
| `--require-complete` | Fail when applicable work cannot be completed |
| `--non-interactive` | Disable prompts and fail when input is required |
| `--retry-failed` | Retry failed, incomplete, and dependency-blocked tasks from the previous run |
| `--no-symbols` | Use ASCII words instead of status symbols |
| `--version` | Print the CLI version |

`--dry-run` applies to mutating commands. Use it first after changing a profile
or configuration.

## Install

```bash
dotfiles install
dotfiles install --profile desktop
dotfiles install --dry-run --verbose
```

`install` is idempotent. Each task inspects current state and applies only the
required change. Independent ready tasks may run concurrently; explicit
dependencies preserve ordering.

Use `--offline` when network access is unavailable or the current checkout must
remain pinned. The normal preservation and sparse-checkout tasks converge the
current checkout without running the post-update reload boundary.

CI automatically enables `--require-complete` and `--non-interactive`.
Non-interactive mode is also enabled when stdin is not a terminal; select a
profile explicitly or through `DOTFILES_PROFILE` in unattended environments.
Strict completion turns missing capabilities such as package managers, VS Code,
APM authentication, or elevation into command failures instead of successful
skips.

Only one command may operate on a repository at a time. A second run reports
the PID, command, and start time of the owner recorded in the repository's
common Git directory.

After a failed run, add `--retry-failed` to the original command to retry
incomplete tasks and their prerequisite closure. Recovery state is structured
and repository-scoped, not reconstructed from display logs. Retry mode cannot
be combined with `--only` or `--skip`.

When `install-arch` runs the desktop profile inside `arch-chroot`, dotfiles
enables user units directly on disk because no user service manager exists yet.
It also records VS Code extension work for
`dotfiles-first-login.service`. The service retries verified Marketplace
installation after the graphical session starts and removes its marker only
after every configured extension converges. Inspect a failed deferred run with:

```bash
journalctl --user -u dotfiles-first-login.service
```

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

Both options can be used together. `--only` limits the candidate set, then
`--skip` removes matches. Matching does not use Rust type names, arbitrary
substrings, action-prefix removal, or the first word of a label.
For example, `repository` and `dotfiles-repository` both match **Dotfiles
repository**, but `dotfiles` does not. Unmatched selectors produce a warning.
Internal orchestration tasks are omitted from discovery and cannot be selected.
When filtering removes a declared prerequisite, the CLI warns and preserves the
targeted-run behavior by assuming that prerequisite is already satisfied.

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

Every run starts with a dimmed header. It shows the command, `dry run` for a
preview, the resolved profile, and the platform. An active overlay adds
`overlay <path>`.

```text
Install · dry run · profile desktop · Arch Linux · overlay ~/src/dotfiles-private
```

Visible task rows are printed as tasks complete, so independent parallel tasks
may appear in a different order between runs. Statuses distinguish the outcome:

| Status | Meaning |
|---|---|
| `✓` | The task applied one or more changes, or a validation task passed |
| `~` | Dry-run changes were planned but not applied |
| `⊘` | The task was skipped |
| `✗` | The task failed |
| `○` | The task was already up to date (verbose only) |
| `⁃` | The task does not apply to this platform or configuration (verbose only) |

Use `--no-symbols` to restore the ASCII status words for terminals or pipelines
that cannot render the glyphs.

A row states the task name and, when the task has something to explain, the
reason after a `·` separator:

```text
⊘ Dotfiles repository · local changes present
```

Indented dimmed lines beneath a row are the individual actions the task took or
planned, listed in full without truncation. They follow their task row
immediately, without blank lines between task outputs.

Normal output includes only tasks that changed state or need attention.
`--verbose` includes every task, including current and non-applicable tasks. It
adds elapsed time to each task that ran and shows the resource decisions behind
each outcome. `⁃` rows have no elapsed time because nothing ran. Internal
orchestration remains in the run log but does not appear in console rows or
totals.

Before the scheduler starts, an installed binary checks for a newer release. The
check draws a transient status line while it runs and erases it afterwards; a
line is left behind only when a new version was actually installed:

```text
Self update · v2025.01.02-1 → v2025.01.09-1
```

While tasks are running, a transient status line reports progress and the
currently active tasks:

```text
Running · 12/16 done · Home symlinks, System packages
```

The counter reports tasks that have already finished; the names after it are the
tasks running right now. Its denominator counts the same tasks the summary
accounts for. Applicability is only known once a task has run, so a task that
turns out not to apply leaves the denominator rather than advancing the
numerator.

The final line reports task counts without status glyphs. Color distinguishes
each outcome group: green for changed, magenta for dry run, dim for current,
yellow for ignored, and red for failed. For example:
`2 changed · 14 current · 1 ignored · 2.3s`,
`5 would change · 8 current · 3 ignored · 0.7s`, or
`6 passed · 1 ignored · 1.4s`.

## Uninstall

```bash
dotfiles uninstall --dry-run
dotfiles uninstall
```

Uninstall does only the following:

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
dotfiles test --only config-warnings,manifest-sync
dotfiles test --overlay C:\path\to\private-dotfiles
```

The command validates TOML, sources, manifest section synchronization, APM
fragments, local plugin references, and MCP entries. When APM is available, a
separate task runs `apm pack --dry-run --verbose` for each local plugin.
ShellCheck and that APM pack check are skipped when their executables are
unavailable. The PowerShell check runs whenever `pwsh` is available; if the
PSScriptAnalyzer module is missing, that check fails and reports the PowerShell
error. Use `--only` or `--skip` with selectors from `dotfiles tasks` to narrow
the validation task set.

## Logs

Every run writes a separate log file and retains the newest 50. The CLI prints
the exact path after each run so an installation script can retain or collect
it. On Linux, the default directory is
`$XDG_STATE_HOME/dotfiles/logs`, or `~/.local/state/dotfiles/logs` when
`XDG_STATE_HOME` is unset; set `DOTFILES_LOG_DIR` to choose another directory.

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

Logs contain every event in execution order, including messages hidden from the
console. Lines are
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

An explicit overlay path that is a linked Git worktree requires interactive
confirmation before it is used or persisted. The `[y/N]` prompt defaults to no;
non-interactive runs reject the new worktree path.

Only overlay repositories can define `conf/scripts.toml`. See
[Configuration overlays](CONFIGURATION.md#overlays).

## Interrupting a run

Ctrl-C escalates rather than repeats:

1. The first Ctrl-C requests cancellation. The engine stops dispatching new work
   and lets in-flight operations finish. This is reported once, no matter how
   long the wait lasts, and the run still writes its summary and log.
2. A second Ctrl-C asks whether to give up on that wait:

   ```text
   Force quit? in-flight operations will be abandoned [y/N]:
   ```

   Answering anything other than `y` or `yes` keeps waiting.
3. Answering `y`, or pressing Ctrl-C again while the question is open, quits
   immediately with exit code 130.

Force quitting skips the shutdown that normally terminates spawned commands.
Child processes may keep running, and state may be partially applied. If output
is redirected or no terminal is attached, the second Ctrl-C force quits without
asking.

## Exit behavior

A command exits unsuccessfully when required configuration cannot be loaded, a
task fails, or validation reports an error. A force quit exits with code 130.
Non-applicable tasks and optional tool checks are recorded separately from
failures. Use `--verbose` and `dotfiles log --verbose` when diagnosing a failed
run.
