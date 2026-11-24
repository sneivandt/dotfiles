---
title: CLI reference
description: Look up dotfiles command flags, wrapper bootstrap, configuration paths, stable run IDs, and JSON Lines output for scripts.
---

# CLI reference

Use this page to look up exact command syntax, flags, wrapper bootstrap rules,
log handling, and machine-readable output. For installation see
[Getting started](../guides/getting-started.md), for everyday workflows see
[Commands and usage](../guides/commands-and-usage.md), and for individual tasks
and selectors see [Task reference](task-reference.md).

Commands below use the installed `dotfiles` launcher and assume a configured
checkout and saved profile. Before installing the launcher, substitute
`./dotfiles.sh` on Linux or `.\dotfiles.ps1` on Windows from the checkout root
(see [Wrapper bootstrap](#wrapper-bootstrap)).

## Command summary

| Command | Purpose |
|---|---|
| `update` (default) | Sync the repository and apply configuration. Installing a missing Arch package can also [upgrade the system](system-packages.md#what-it-does). |
| `remove` | Detach a limited set of integrations (not a full restore to machine defaults) |
| `check` | Validate configuration and repository files |
| `list` | Discover task selectors or inspect a dependency graph |
| `explain` | Inspect the declaration and planned actions for a [managed home symlink](../guides/commands-and-usage.md#inspect-a-home-path) |
| `log` | Read saved run logs |
| `help` | Show general usage, or pass a command name for its flags |

## Command options

Flags can appear before or after the command name (omitting a command runs
`update`). Run `dotfiles <command> --help` for full syntax, and
`dotfiles --version` or `-V` for the CLI version.

### Configuration selection

Available on `update`, `remove`, `check`, `list`, and `explain`:

| Option | Purpose |
|---|---|
| `-p`, `--profile <name>` | Choose `base` or `desktop` for this run |
| `--root <path>` | Read configuration from a specific checkout |
| `--overlay <path>` | Include private or machine-specific configuration |

### Task selection and previews

| Option | Where it applies |
|---|---|
| `--only <selectors>` | Include tasks in `update`, `remove`, `check`, or a `list` graph |
| `--skip <selectors>` | Exclude tasks in those commands; takes precedence over `--only` |
| `--with-deps` | Include dependencies for `update` or an `update` graph |
| `-n`, `--dry-run` | Preview `update` or `remove` |
| `--no-repo-update` | Omit repository synchronization from `update` |
| `--restore-missing` | With `remove`, also create source copies at missing configured home targets |

Pass selectors as comma-separated values or repeated flags. Combining
`--only repository` with `--no-repo-update` is an error.

### Execution and output

Available on `update`, `remove`, and `check`:

| Option | Purpose |
|---|---|
| `-v`, `--verbose` | Include unchanged tasks, decisions, and timings (also accepted by `log`) |
| `-q`, `--quiet` | Show only warnings and incomplete outcomes |
| `--format human\|jsonl` | Choose human-readable or JSON Lines output |
| `--no-parallel` | Run selected work sequentially |
| `--non-interactive` | Disable interactive prompts |
| `--fail-on-skip` | Fail if applicable tasks cannot complete |
| `--no-symbols` | Use text status labels |

`update` and `remove` also accept `--skip-attestation` to bypass CLI
self-update provenance checks (never SHA-256 checksums). That flag doesn't
affect wrapper bootstrap; set `DOTFILES_SKIP_ATTESTATION=1` to bypass
provenance at both layers. Treat neither as a routine workaround for
verification failures.

### Discovery and history

| Option | Where it applies |
|---|---|
| `--format table\|plain\|json` | Task listing, graphs, or `log --list` |
| `--graph update\|remove\|check` | Inspect a command graph with `list` |
| `--all` | Include inapplicable tasks in `list` (cannot combine with `--graph`) |

See [Task discovery output](#task-discovery-output) for graph notation and
[Logs](#logs) for log filtering.

## Repository and overlay paths

`--root .` targets your current directory. Without `--root`, `dotfiles` checks
`DOTFILES_ROOT`, paths relative to its own executable, and then the current
directory (a directly invoked `bin/dotfiles` prefers its own checkout even when
nested inside another one, and wrappers set `DOTFILES_ROOT` to their checkout).

`--root` selects configuration, **not the executable**: an installed launcher
still invokes the binary from its own checkout. Run the target checkout's
wrapper or built binary directly when you need a specific executable.

Use `--overlay <PATH>` for private or machine-specific additions. `update`,
`remove`, and `check` remember an explicit overlay for subsequent runs (even
under `--dry-run`), whereas `list` and `explain` use it for that single call.
See [Private overlays](../guides/private-overlays.md) for setup, path
precedence, worktree confirmation, merging, and scripts.

## Wrapper bootstrap

The repository wrappers fetch or locate the CLI binary and forward your
arguments:

```bash
./dotfiles.sh --version
```

```powershell
.\dotfiles.ps1 --version
```

Both print the version without requiring a profile. If no cached binary is
available, the wrapper downloads a release into `bin/`, checks its SHA-256
checksum (using `flock` from `util-linux` on Linux to avoid concurrent installs),
and verifies build provenance via `gh`. Missing `gh` or failed verification
stops bootstrap unless `DOTFILES_SKIP_ATTESTATION=1` is explicitly set (see
[Build provenance verification](../guides/security-and-trust.md#build-provenance-verification)).

**Even wrapper `--help`, `list`, and `--dry-run` can trigger a download.**
Invoke an existing binary directly if you want to skip bootstrap.

The release publisher comes from Git's `origin` remote unless overridden by
`DOTFILES_RELEASE_REPO=owner/repo`; missing or unsupported remotes fail loudly
instead of falling back to another repo. Forks without published releases need
an explicit publisher override or a
[source build](../development/contributing.md#source-builds); see
[fork setup](../guides/getting-started.md#using-a-fork) for both options.
`--build` compiles from source for that single invocation only.

Release binaries embed their publisher for future self-updates, so changing
`origin` or `DOTFILES_RELEASE_REPO` later does not retarget an existing binary.
Normal development builds never self-update.

Once `launcher` and `path` succeed, open a new shell. The installed launcher
in `~/.local/bin` delegates back to your checkout rather than standing alone,
so keep your checkout at a stable path.

## CLI self-update

Self-update runs only for a release binary living in the selected checkout's
`bin/` directory with an embedded publisher (changing `origin` never retargets
it, and dev builds never self-update).

After passing SHA-256 and provenance checks, the candidate binary must pass a
`--version` smoke test; if it fails, the CLI restores the previous binary (and
reports any rollback error). Failed verification rejects the download outright.
Release lookups cache fresh metadata for ten minutes; `--dry-run` reports
available updates without installing or writing cache files. See
[Release downloads](../guides/security-and-trust.md#release-downloads) for
attestation rules and bypasses.

## Task discovery output

```bash
dotfiles list
dotfiles list --all
dotfiles list --graph update
dotfiles list --format json
```

By default, `dotfiles list` groups selectors by `update`, `remove`, and `check`
and hides tasks that cannot apply to your platform or config; `--all` shows
everything. Listing a task doesn't check whether its tools are installed or
whether anything will change.

Table output sorts selectors alphabetically within each group, while `plain`
(tab-separated) and `json` list each selector once with its command membership.
You can't combine `--graph` with `--all`, and `list` uses `--skip repository`
instead of `--no-repo-update`.

The human graph prints one block per task in dependency order (including
internal tasks), labeling each as `default`, `requested`, `dependency`,
`filtered`, or `skipped`. `requires <-` means a predecessor must succeed;
`after <-` waits without inheriting failure. Filters annotate excluded nodes
instead of hiding them, and internal tasks cannot be selected directly.

`list` requires loadable configuration and a saved (or explicit) profile and
never prompts interactively (explicit linked-worktree overlays still require
their confirmation).

## Logs

After a failure, run the exact `dotfiles log --id ...` command printed by the
CLI. Run IDs stay stable across later runs, unlike numeric history indexes; in
`log --list`, copy the `ID` beside a timestamp, and check `PARENT` to see if a
run was spawned by a restart or elevation.

```bash
dotfiles log                          # newest run
dotfiles log --verbose                # newest run, with diagnostics
dotfiles log --list                   # history and stable run IDs
dotfiles log --list --format json
dotfiles log 2                        # third-newest run
dotfiles log --task symlinks --verbose # one task in the newest run
dotfiles log --raw                    # original stored records
```

For a specific failure, run
`dotfiles log --id <run-id> --task <selector> --verbose` (for example
`--task symlinks`). `--id` cannot combine with a numeric index, `--list`, or
`--command`, and `--verbose` conflicts with `--raw`.

`--command update`, `remove`, or `check` filters history before indexing
(historical `install` and `uninstall` filters are still accepted for old logs).

Default log output shows task outcomes, actions, warnings, and failed-command
output; add `-v` for passing-check counts, task starts, resource operations,
timings, and event IDs. Both views escape NUL bytes as `\0` and keep
unrecognized lines visible. `--raw` dumps the stored records verbatim, though
routine successful stdout isn't saved in the first place, and oversized command
output is marked truncated rather than treated as a complete inventory.

Restarted or elevated child processes write their own logs linked via `PARENT`,
and `unfinished` indicates no finish record yet (the process may still be
running or was killed). Retention keeps the newest 50 Dotfiles logs without
deleting active ones or unrelated files, and reading logs never starts a new
logged run.

### Log locations

`DOTFILES_LOG_DIR` overrides the log folder (relative paths allowed). Otherwise
the defaults are:

- Linux: `~/.local/state/dotfiles/logs`
- Windows: `%USERPROFILE%\AppData\Local\dotfiles\logs`

An absolute `XDG_STATE_HOME` (Linux) or `LOCALAPPDATA` (Windows) replaces the
state base directory (appending `dotfiles/logs`), with `./dotfiles/logs` as the
final fallback if standard variables are empty or relative. An unreadable log
directory raises an error rather than showing empty history. **Check logs for
private paths, config values, and tool output before sharing them.**

## Execution output for scripts

Quiet mode (`-q`) preserves logs and exit codes while sending warnings and
failed, blocked, skipped, or interrupted rows to stderr (conflicts with
`--verbose` and `--format`).

For scripted automation, use JSON Lines (`--format jsonl`):

```bash
dotfiles check --profile base --only config-diagnostics --format jsonl
```

Wrapper bootstrap logs go to stderr. Each stdout line is a JSON object with
`schema: 1`, `run_id`, `context`, and `type` (`run_start`, `run_scope`,
`run_context`, `task_result`, `task_duration`, `message`, or `run_finish`).
`task_result` carries IDs, selectors, stored statuses (`ok`, `dry_run`,
`not_applicable`, etc.), reasons, and action counts; `run_finish` carries
outcome, elapsed microseconds, and exit code.

This excerpt comes from an isolated passing config check (run/task IDs and
elapsed time are normalized, and intermediate scope/context/duration/diagnostic
lines are omitted):

```json title="JSON Lines (fixture excerpt)"
{"schema":1,"run_id":"fixture-run","context":"main","type":"run_start","command":"check","parent_run_id":null}
{"schema":1,"run_id":"fixture-run","context":"fixture-task","type":"task_result","task_id":"fixture-task","selector":"config-diagnostics","name":"Configuration diagnostics","status":"passed","reason":null,"actions":{"applied":0,"planned":0,"skipped":0,"failed":0,"interrupted":0,"not_attempted":0}}
{"schema":1,"run_id":"fixture-run","context":"main","type":"run_finish","outcome":"succeeded","elapsed_us":0,"exit_code":0}
```

| Field | Type and consumer contract |
|---|---|
| `schema` | Integer envelope version; this format uses `1` |
| `run_id` | Opaque string identifying this started process; group records by it |
| `context` | String identifying the emitting context, not a task selector |
| `type` | String determining the record's remaining fields |
| `parent_run_id` | String or `null` on a start record; links a restarted/elevated child to its parent |
| `task_id`, `selector` | Opaque task identity and public selector; internal task records may omit `selector` |
| `status`, `reason` | Stored status string and a string or `null` explaining the result |
| `actions` | Object of integer item counts; these are not task totals |
| `outcome`, `exit_code`, `elapsed_us` | Finish outcome string, integer process code, and integer elapsed microseconds |

Match tasks by `selector` rather than internal `task_id` or `context`, ignore
extra fields and unknown record types, and fail cleanly on an unrecognized
`schema`. The
[documentation contract fixture](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/docs_contracts.rs)
verifies this excerpt against real CLI output.

JSON Lines disables interactive prompts, keeps parse errors and logging
warnings on stderr, and writes detailed subprocess output to logs. Group
records by `run_id` (restarted or elevated children have their own IDs linked
via `parent_run_id`) and tolerate a missing `run_finish` after a hard kill.

**Closing stdout does not cancel a run:** a broken pipe stops stdout writes,
but managed work continues to completion.

## Exit behavior

| Exit code | Meaning |
|---|---|
| `0` | No failures; skipped work is allowed unless strict skip handling is enabled |
| `1` | Configuration/runtime failure, task failure, or incomplete applicable work with `--fail-on-skip` or `CI` |
| `2` | Invalid command-line usage |
| `130` | Interrupted; a genuine task failure takes precedence during graceful shutdown |

To look up individual task selectors and behaviors, continue to
[Task reference](task-reference.md), or see
[Troubleshooting](../guides/troubleshooting.md) for failure recovery.
