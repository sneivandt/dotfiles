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

By default, a wrapper uses `bin\dotfiles` or `bin\dotfiles.exe`. If the binary
is absent, it downloads the latest compatible GitHub Release asset and verifies
its SHA-256 checksum. Use wrapper-only `--build` to compile the CLI with Cargo:

```bash
./dotfiles.sh --build install --dry-run
```

```powershell
.\dotfiles.ps1 --build test
```

After installation, `~\.local\bin\dotfiles` is the normal entry point.

## Command summary

| Command | Behavior |
|---|---|
| `install` | Converges the configured machine state without advancing pinned dependency versions |
| `update` | Runs the install graph and includes version-advancing update tasks |
| `uninstall` | Removes managed integrations while preserving user files and broader machine state |
| `test` | Validates configuration and runs available script analyzers |
| `tasks` | Lists visible task selectors, labels, and command membership |
| `log` | Prints the latest run log |
| `completions <shell>` | Hidden support command that emits shell completion definitions |

## Global options

Global options may be placed before or after the subcommand.

| Option | Meaning |
|---|---|
| `-v`, `--verbose` | Show complete task and action details |
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

Visible task rows are printed as tasks complete, so independent parallel tasks
may appear in a different order between runs. Statuses distinguish the outcome:

| Status | Meaning |
|---|---|
| `CHANGE` | The task applied one or more changes |
| `DRYRUN` | Dry-run changes were planned but not applied |
| `PASSED` | A validation task passed |
| `IGNORE` | The task was intentionally ignored |
| `FAILED` | The task failed |

Changed, planned, ignored, and failed rows include useful indented, dimmed
details. Normal output shows at most eight detail lines and then points to
`-v`; verbose mode shows complete messages for emitted task rows. Tasks that
require no change and tasks that are not applicable do not emit rows in either
mode. Internal orchestration remains available in diagnostic logs but is
excluded from normal rows and totals.

The final line reports actual action counts and affected task counts, for
example `Applied 4 changes across 2 tasks · 1 ignored · 2.3s` or
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

```bash
dotfiles log
dotfiles log --verbose
```

The verbose form prefers the diagnostic log. If no diagnostic log is available,
it falls back to the normal run log.

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

Only overlay repositories can define `conf\scripts.toml`. See
[Configuration overlays](CONFIGURATION.md#overlays).

## Exit behavior

A command exits unsuccessfully when required configuration cannot be loaded, a
task fails, or validation reports an error. Non-applicable tasks and optional
tool checks are recorded separately from failures. Use `--verbose` and
`dotfiles log --verbose` when diagnosing a failed run.
