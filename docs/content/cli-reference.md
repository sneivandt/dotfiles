---
title: CLI reference
description: Look up command options, wrapper bootstrap, configuration paths, log syntax, and structured output.
---

# CLI reference

This is the syntax and behavior reference for the dotfiles command-line program.
For a first installation, use [Getting started](getting-started.md); for common
workflows, use [Commands and usage](commands-and-usage.md).

Examples using the installed launcher assume a configured checkout and saved
profile. Before installation, use that checkout's `./dotfiles.sh` on Linux or
`.\dotfiles.ps1` on Windows. [Wrapper bootstrap](#wrapper-bootstrap) explains
downloads and the source-build route. Task selectors and their managed state
are listed in [Task reference](task-reference.md).

## Command summary

| Command | Purpose |
|---|---|
| `update` (default) | Apply configured state and update dependencies when their version rules allow it |
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
./dotfiles.sh list --profile base
```

```powershell
.\dotfiles.ps1 list --profile desktop
```

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
[fork instructions below](#using-a-fork) explain both routes. `--build` selects
source compilation for that invocation only. Without it, the wrapper uses its
download/cached-binary route.

Release binaries retain their publishing repository for self-update.
Changing `origin` or the bootstrap override does not redirect an existing
binary's updates. Normal development builds do not self-update.

After the `launcher` and `path` tasks succeed, open a new shell. The installed
launcher in `~/.local/bin` delegates back to this checkout; it is not a
standalone installation. Keep the checkout at a stable path.

### Using a fork

**A new fork does not have release binaries.** Choose one of these routes
before running its wrapper:

- **Use this project's released CLI with your fork's configuration.** If you
  trust this project's releases, explicitly select that publisher in your
  current terminal:

  ```sh
  export DOTFILES_RELEASE_REPO=sneivandt/dotfiles
  ```

  In PowerShell:

  ```powershell
  $env:DOTFILES_RELEASE_REPO = 'sneivandt/dotfiles'
  ```

  This chooses the binary, not the configuration repository. The downloaded
  binary also uses that publisher for self-updates. If your fork publishes
  compatible releases, you can use its default publisher instead.

- **Build the CLI from your fork.** Install the
  [source-build prerequisites](contributing.md#development-setup), then add
  `--build` after the wrapper name in each command you run. For example,
  `./dotfiles.sh --build --version` or
  `.\dotfiles.ps1 --build --version` builds and prints the version without
  applying configuration. Build mode is not remembered between runs.

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

Graph output shows every node, including internal tasks, with selection labels:
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
| `0` | Succeeded under the selected skip policy |
| `1` | Configuration/runtime failure, task failure, or unmet work under strict policy |
| `2` | Invalid command-line usage |
| `130` | Interrupted; a genuine task failure takes precedence during graceful shutdown |

For recovery, use [Troubleshooting](troubleshooting.md).
