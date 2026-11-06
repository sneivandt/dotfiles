---
title: CLI reference
description: Look up dotfiles command flags, wrapper bootstrap, configuration paths, stable run IDs, and JSON Lines output for scripts.
---

# CLI reference

Look up exact command syntax and output here. Use
[Getting started](getting-started.md) for installation,
[Commands and usage](commands-and-usage.md) for everyday procedures, and
[Task reference](task-reference.md) for selectors and the state each task manages.

Examples using the installed launcher assume a configured checkout and saved
profile. Before installation, use that checkout's `./dotfiles.sh` on Linux or
`.\dotfiles.ps1` on Windows. [Wrapper bootstrap](#wrapper-bootstrap) explains
downloads and the source-build route.

## Command summary

| Command | Purpose |
|---|---|
| `update` (default) | Apply configured state and update dependencies when their version rules allow it |
| `remove` | Detach a limited set of integrations, not restore machine defaults |
| `check` | Validate configuration and repository files |
| `list` | Discover selectors or inspect a dependency graph |
| `explain` | Inspect the declaration and planned actions for a [managed home symlink](commands-and-usage.md#inspect-a-home-path) |
| `log` | Read saved run logs |
| `help` | Show general usage, or pass a command name to see its usage |

## Command options

Options may appear before or after an explicit command. With no command, they
apply to `update`. Use `dotfiles <command> --help` for full syntax, and
`dotfiles --version` or `-V` for the version.

### Configuration selection

These options apply to update, remove, check, list, and explain:

| Option | Purpose |
|---|---|
| `-p`, `--profile <name>` | Choose `base` or `desktop` for this run |
| `--root <path>` | Read configuration from a specific checkout |
| `--overlay <path>` | Include private or machine-specific configuration |

### Task selection and previews

| Option | Where it applies |
|---|---|
| `--only <selectors>` | Include tasks in update, remove, check, or a list graph |
| `--skip <selectors>` | Exclude tasks in those commands; takes precedence over `--only` |
| `--with-deps` | Include dependencies for update or an update graph |
| `-n`, `--dry-run` | Preview update or remove |
| `--no-repo-update` | Omit repository synchronization from update |
| `--restore-missing` | With remove, also create source copies at missing configured home targets |

Selectors can be comma-separated or supplied with repeated flags.
Selecting `repository` with both `--only` and `--no-repo-update` is an error.

### Execution and output

These options apply to update, remove, and check:

| Option | Purpose |
|---|---|
| `-v`, `--verbose` | Include current tasks, decisions, and timings; also available in log viewing |
| `-q`, `--quiet` | Keep warnings and incomplete outcomes |
| `--format human\|jsonl` | Choose human-readable or JSON Lines output |
| `--no-parallel` | Run selected work sequentially |
| `--non-interactive` | Disable prompts |
| `--fail-on-skip` | Fail if applicable tasks cannot complete |
| `--no-symbols` | Use text status labels |

Update and remove also accept `--skip-attestation` to bypass CLI self-update
provenance checks, never checksums. It does not affect wrapper bootstrap;
`DOTFILES_SKIP_ATTESTATION=1` affects both layers. Neither is a routine workaround
for verification errors.

### Discovery and history

| Option | Where it applies |
|---|---|
| `--format table\|plain\|json` | Task listing, graphs, or `log --list` |
| `--graph update\|remove\|check` | Inspect a command graph with list |
| `--all` | Include inapplicable tasks in list; cannot combine with `--graph` |

See [Task discovery output](#task-discovery-output) for graph markers and
[Logs](#logs) for run selection and filtering.

## Repository and overlay paths

`--root .` selects the checkout you are standing in. Without `--root`, dotfiles
tries `DOTFILES_ROOT`, candidate paths relative to its executable, then the
current directory. Wrappers set `DOTFILES_ROOT` to their own checkout.

`--root` chooses configuration, **not the executable**. An installed launcher
can still obtain a binary from another checkout. Use the intended checkout's
wrapper or built binary when that distinction matters.

Use `--overlay <PATH>` for private or machine-specific additions. Unlike
`--profile`, an explicit overlay is saved for future runs, including during
check or dry-run; `list` does not save it. See [Private overlays](private-overlays.md)
for setup, path precedence, linked-worktree confirmation, merging, and scripts.

## Wrapper bootstrap

The repository wrappers obtain the CLI and forward its arguments:

```bash
./dotfiles.sh --version
```

```powershell
.\dotfiles.ps1 --version
```

These commands print the CLI version without needing a profile selection.
If a usable cached binary is absent, the wrapper downloads a release into
`bin/` and verifies its SHA-256 checksum. It also verifies build provenance
when GitHub CLI is available. A missing GitHub CLI produces a warning; failed
verification with the tool present blocks bootstrap. See
[Build provenance verification](security-and-trust.md#build-provenance-verification).

**Even wrapper help, list, and dry-run can download files.** Use an existing
binary directly to avoid bootstrap.

The publisher comes from the checkout's GitHub `origin` remote unless
`DOTFILES_RELEASE_REPO=owner/repo` explicitly selects another publisher.
Unsupported/missing remotes fail instead of falling back to this project's
releases. Forks without release assets need a deliberate publisher choice or
a [source build](contributing.md#source-builds). The
[fork setup](getting-started.md#using-a-fork) explains both routes. `--build` selects
source compilation for that invocation only. Without it, the wrapper uses its
download/cached-binary route.

Release binaries retain their publishing repository for self-update.
Changing `origin` or the bootstrap override does not redirect an existing
binary's updates. Normal development builds do not self-update.

After the `launcher` and `path` tasks succeed, open a new shell. The installed
launcher in `~/.local/bin` delegates back to this checkout; it is not a
standalone installation. Keep the checkout at a stable path.

## CLI self-update

Release self-update runs only for a binary in the selected checkout's `bin/`
directory with an embedded release publisher. Changing Git `origin` does not
redirect that existing binary to a different publisher. Development builds do
not self-update.

After checksum and provenance verification, the replacement must pass a
`--version` smoke test. A failed smoke test attempts to restore the previous
binary and reports rollback failures. Failed verification rejects the download.
Release lookups may reuse fresh results for ten minutes; a dry run can report
an available version but does not install it or write release caches.
See [Release downloads](security-and-trust.md#release-downloads) for attestation
requirements and bypass controls.

## Task discovery output

```bash
dotfiles list
dotfiles list --all
dotfiles list --graph update
dotfiles list --format json
```

The normal listing groups selectors by update, remove, and check. It hides
tasks whose platform or configuration clearly rules them out; `--all` includes
them. A visible task is a possibility, not proof that its tools are installed
or that anything needs changing.

Table output sorts selectors alphabetically within each command group.
Plain/JSON listings contain each selector once, with its command membership.
Graphs cannot be combined with `--all`; use `--skip repository` to exclude
repository work in a graph because `list` does not accept `--no-repo-update`.

The human graph uses one block per task, with predecessors first. It shows
every node, including internal tasks, with selection labels:
`default`, `requested`, `dependency`, `filtered`, or `skipped`.
`requires <-` means the predecessor must succeed; `after <-` means wait for it
without inheriting its failure. Filters annotate the graph rather than hiding
its excluded nodes. Internal tasks cannot be selected directly.

Use `--format plain` for tab-separated records or `--format json` for JSON.
`list` still needs loadable configuration and an existing profile selection;
it never opens the profile prompt. Explicit linked-worktree overlays retain
their separate confirmation requirement.

## Logs

After a failure, use the exact log-viewing command printed by the CLI. It
selects the run with `--id` and adds `-v` for diagnostics. IDs stay stable even
after later runs; numeric history indexes do not.

```bash
dotfiles log                          # newest run
dotfiles log --verbose                # newest run, with diagnostics
dotfiles log --list                   # history and stable run IDs
dotfiles log --list --format json
dotfiles log 2                        # third-newest run
dotfiles log --task symlinks --verbose # one task in the newest run
dotfiles log --raw                    # original stored records
```

For a known failure, prefer
`dotfiles log --id <run-id> --task symlinks --verbose`, replacing `<run-id>`
with its actual history ID. To narrow the log, pass the task selector
to `--task` (the example uses `symlinks`). Add `--verbose` for diagnostics or
`--raw` for the original records. `--id` cannot be combined with an index, `--list`, or
`--command`; verbose and raw are mutually exclusive.

`--command update`, `remove`, or `check` filters history before selecting an
index. Historical `install` and `uninstall` filters remain supported; they
are not current execution commands.

Normal viewing shows actions, outcomes, warnings, context, and failed-command
output. Verbose adds task starts, checks, timings, and debug details.
An external command can return a nonzero exit code without failing its task
when that code has an expected meaning, such as "not installed."
Successful stdout is usually not saved;
`--raw` cannot recover output that was never retained. Oversized subprocess
output is marked truncated and is not accepted as a complete inventory.

Each restarted or elevated process has a separate log linked to its parent.
`unfinished` means no finish record exists; the process may still be running.
Retention targets the newest 50 logs without deleting active ones. Reading
logs does not start another logged run.

### Log locations

`DOTFILES_LOG_DIR` overrides the location, including with a relative path.
Otherwise, the defaults are:

- Linux: `~/.local/state/dotfiles/logs`
- Windows: `%USERPROFILE%\AppData\Local\dotfiles\logs`

An absolute `XDG_STATE_HOME` on Linux or `LOCALAPPDATA` on Windows replaces
the state-directory base shown above; logs go in its `dotfiles/logs` subdirectory.
Empty or relative standard state/home variables are ignored. The final
fallback is `./dotfiles/logs`. An unreadable directory is an error, not empty
history. **Review logs for private paths, configuration, and tool output before
sharing them.**

## Execution output for scripts

Quiet mode preserves logs and exit policy. Warnings and failed, blocked,
skipped, or interrupted rows remain on stderr. It conflicts with `--verbose`
and an explicit `--format`.

For a focused, non-interactive diagnostic stream, use JSON Lines:

```bash
dotfiles check --profile base --only config-diagnostics --format jsonl
```

Each stdout line is one object with `schema: 1`, `run_id`, `context`, and
`type`. Record types include `run_start`, `run_scope`, `run_context`,
`task_result`, `task_duration`, `message`, and `run_finish`. Task results carry
IDs, selectors, statuses, reasons, and action counts; finish records include
outcome and exit code. Stored statuses use names such as `ok`, `dry_run`, and
`not_applicable`, rather than the human display labels.

This JSON Lines excerpt comes from an isolated successful configuration check.
The regression normalizes run/task identities and elapsed time for comparison.
Scope, context, duration, and diagnostic records between these lines are omitted:

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

Use selectors for user-facing task matching, not internal task identities or
context names. Parse only the fields needed for your record type, tolerate
additional fields and unknown types, and report an unsupported schema version.
The [documentation contract fixture](https://github.com/sneivandt/dotfiles/blob/main/cli/tests/docs_contracts.rs)
compares this excerpt with real CLI output, not separately invented records.

JSON Lines disables prompts. Parse errors and logging warnings can still
appear on stderr; detailed actions and subprocess diagnostics remain in logs.
Group records by run ID: restarted/elevated children have their own IDs and
`parent_run_id` links. Tolerate additional fields/types and missing finish
records after a force quit.

**Closing stdout does not cancel an update.** A closed pipe stops further
stdout output, but managed work continues.

## Exit behavior

| Exit code | Meaning |
|---|---|
| `0` | No failures; skipped work is allowed unless strict skip handling is enabled |
| `1` | Configuration/runtime failure, task failure, or incomplete applicable work with `--fail-on-skip` or CI |
| `2` | Invalid command-line usage |
| `130` | Interrupted; a genuine task failure takes precedence during graceful shutdown |

For recovery, use [Troubleshooting](troubleshooting.md).
