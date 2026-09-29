# Usage

Use this guide to preview, apply, and inspect the repository's configuration.
The wrappers obtain the Rust CLI; the CLI selects configuration and runs tasks.
For configuration syntax, see [Configuration](CONFIGURATION.md). For the work
each task performs, see [Task reference](TASKS.md).

## Bootstrap

Start in the checkout you intend to keep. Installed home links point into this
checkout, so moving or deleting it later can break applications.

Before applying anything:

1. Choose `base` for a shell-focused machine or `desktop` for a workstation.
   These are roles, not safety levels; both can change system settings.
   See [Profiles](PROFILES.md).
2. Review the selected configuration and any private overlay.
3. Back up existing home configuration. **Symlink installation can replace
   existing ordinary files and empty directories without a backup.** A warning
   is not a confirmation prompt. Nonempty directories cause a failure rather
   than being recursively removed.
4. Discover tasks, then preview the current checkout without synchronizing it.

### Linux

Run from the repository root:

```bash
# May download the CLI; task discovery itself does not apply configuration.
./dotfiles.sh tasks --root . --profile base

# Use the downloaded binary directly and suppress its release check.
DOTFILES_SKIP_SELF_UPDATE=1 ./bin/dotfiles install \
  --root . --profile base --no-repo-update --dry-run --verbose
```

After reviewing the preview and backing up affected files, this **applies** the
current checkout:

```bash
DOTFILES_SKIP_SELF_UPDATE=1 ./bin/dotfiles install \
  --root . --profile base --no-repo-update
```

### Windows

Run from the repository root in PowerShell, normally **not** as Administrator:

```powershell
# May download the CLI.
.\dotfiles.ps1 tasks --root . --profile desktop

# Previews this checkout; does not apply task changes.
.\bin\dotfiles.exe install --root . --profile desktop --no-repo-update --dry-run --verbose
```

The preview can still check for a CLI release and write its cache. See
[Dry-run scope](#dry-run-scope) and the [Windows guide](WINDOWS.md) before
applying changes or responding to elevation prompts.

### What the wrapper does

A wrapper uses `bin/dotfiles` or `bin\dotfiles.exe`. If the usable binary is
absent, it downloads a compatible GitHub Release asset and verifies SHA-256.
Initial bootstrap verifies build provenance when `gh` is available; when it is
absent, bootstrap warns and continues. Verification failure with `gh` present
blocks the download. See [Build provenance verification](SECURITY.md#build-provenance-verification).

**Even `--help`, `tasks`, or `--dry-run` can trigger a wrapper download.**
Bootstrap runs before the CLI parses those arguments. It uses the network and
writes the binary and download files. Once bootstrapped, invoke the binary
directly for discovery without that bootstrap step.

To build instead of downloading, use the wrapper-only `--build` switch:

```bash
./dotfiles.sh --build tasks --root . --profile base
```

```powershell
.\dotfiles.ps1 --build tasks --root . --profile desktop
```

This requires Cargo and the [build prerequisites](CONTRIBUTING.md). It writes
build artifacts and may download dependencies. The wrapper builds with the
`dev-opt` profile and executes Cargo's reported artifact, including custom
`CARGO_TARGET_DIR` or target-triple locations. It does not need to copy that
artifact into `bin/`.

After the `launcher` and `path` tasks converge, open a new shell and use
`dotfiles`. The launcher is `~/.local/bin/dotfiles` on Linux and
`%USERPROFILE%\.local\bin\dotfiles.cmd` on Windows; it delegates to the
repository wrapper rather than being a standalone binary.

## Command summary

| Command | Use it to |
|---|---|
| `install` | Apply selected desired state, without requesting APM pin advancement |
| `update` | Run the install pipeline with APM pin updates enabled |
| `uninstall` | Materialize home links and remove a limited set of integrations, **not** restore the original machine |
| `check` | Validate configuration and run available repository analyzers |
| `tasks` | Discover selectors or inspect a command's dependency graph without executing tasks |
| `log` | Read retained run logs without starting a new run |
| `help [command]` | Inspect command-specific usage |
| `completions <shell>` | Emit runtime completion registration; hidden support command |

`update` is equivalent to `install --update`. It is **not** a general
operating-system upgrade command: packages already present are not selected
just to upgrade them. However, **installing missing Arch packages invokes
`pacman -Syu --needed --noconfirm`**, so even ordinary `install` can upgrade
other system packages as part of that transaction.

## Command options

Put options **after their command**. They are not global switches.
`dotfiles --version` prints the version without loading configuration; it has
no short alias. `dotfiles <command> --help` gives that command's full syntax.

| Options | Accepted by | Meaning |
|---|---|---|
| `-p`, `--profile <PROFILE>`; `--root <PATH>`; `--overlay <PATH>` | install, update, uninstall, check, tasks | Select role, source checkout, and overlay |
| `-v`, `--verbose` | install, update, uninstall, check, log | Include diagnostic output; for log viewing, incompatible with `--raw` or `--list` |
| `--no-parallel` | install, update, uninstall, check | Run work sequentially |
| `--non-interactive` | install, update, uninstall, check | Disable normal execution prompts; fail if required selection is unavailable |
| `--fail-on-skip` | install, update, uninstall, check | Fail when applicable work remains unmet, including missing optional check tools |
| `--no-symbols` | install, update, uninstall, check | Use ASCII status words |
| `-n`, `--dry-run` | install, update, uninstall | Preview task changes rather than apply them |
| `--only <SELECTOR>`; `--skip <SELECTOR>` | install, update, uninstall, check; tasks with `--graph` | Select or exclude tasks; repeat flags or use comma-separated values |
| `--with-deps` | install, update; tasks with an install/update graph | Include predecessors of `--only` selections; requires `--only` |
| `--no-repo-update` | install, update | Omit synchronization of the main checkout and Git overlay |
| `--update` | install, update | Request APM pin advancement; already implied by `update` |
| `--skip-attestation` | install, update, uninstall | Explicitly bypass self-update provenance verification, not checksum verification |
| `--format table\|plain\|json` | tasks; log with `--list` | Format discovery/history output |
| `--graph install\|update\|uninstall\|check` | tasks | Inspect that command's graph |

`--skip-attestation` is a CLI self-update option, not a bootstrap option.
`DOTFILES_SKIP_ATTESTATION=1` bypasses provenance checks in both layers. Neither
is a routine fix for verification errors.

## Dry-run scope

`--dry-run` suppresses the CLI's planned configuration mutations. It is not a
sandbox, an offline mode, or a promise of zero filesystem writes.

| Activity | During a dry run |
|---|---|
| Wrapper bootstrap or `--build` | Still downloads/builds before the CLI starts |
| CLI self-update | Can query releases and write `bin/.dotfiles-version-cache`; does not replace the binary |
| Repository task | Can query remote refs; does not fetch or merge. Omit it with `--no-repo-update` |
| Run bookkeeping | Writes logs and acquires the repository lock |
| Selection | Interactive profile selection and explicit `--overlay` can persist local Git configuration |
| State inspection | Queries files, package providers, and other external tools; their own caches/side effects are not sandboxed |
| Overlay scripts | Executes `--check` and, when needed, `--dryrun`; safety depends on the script honoring those modes |

For task/graph discovery without executing probes, use an **existing binary's**
`tasks` command. It creates no run log or lock and does not persist selections.
For a focused machine preview, use `--only`, an explicit `--root` and
`--profile`, and `--no-repo-update`. Review overlay scripts before including
them.

## Install

After bootstrap, these examples assume the current directory is the intended
checkout and `dotfiles` is on PATH:

```bash
dotfiles install --root . --profile desktop --no-repo-update --dry-run --verbose
```

Remove `--dry-run` only when ready to apply. Built-in tasks inspect current
state and avoid unnecessary changes; this does not make first-run replacement
of existing configuration harmless. Applications may auto-reload changed
configuration immediately.

Three independent sources of change matter:

| Source | Default behavior | How to keep it fixed for a run |
|---|---|---|
| CLI binary | Release binaries running from the checkout's `bin/` check before install/update/uninstall | Set `DOTFILES_SKIP_SELF_UPDATE=1` |
| Repository content | install/update synchronize the main checkout and eligible Git overlay | Pass `--no-repo-update` |
| APM pins | Ordinary install uses existing pin intent; update enables advancement | Use `install`, not `update` or `--update` |

`--no-repo-update` alone is not network-free. Packages, APM, self-update, and
overlay scripts can still access the network. `--only` limits scheduled tasks;
it does not disable self-update, configuration loading, or startup bookkeeping.
Selecting `repository` with both `--only` and `--no-repo-update` is an error.

Repository updates use fetch and fast-forward-only merge, not a reset or
automatic conflict resolution. If content changes, a guarded child reloads
configuration and rediscovers overlay tasks with the original arguments.
The parent retains the run lock until it exits.

Release checks are cached for up to one hour and, on Linux, only for the boot
that created the cache. Source builds do not self-update from their Cargo
output directory. A restarted child does not repeat the self-update check.

### Select tasks

Discover before filtering:

```bash
dotfiles tasks --root . --profile desktop
dotfiles install --root . --profile desktop --no-repo-update --only symlinks --dry-run
dotfiles install --root . --profile desktop --no-repo-update --only "packages,git-hooks" --dry-run
```

Selectors match exactly after case, punctuation, and whitespace normalization.
`repository` and the full label `dotfiles-repository` both match **Dotfiles
repository**; `dotfiles` does not. Internal orchestration tasks cannot be
selected. Unknown selectors and an explicit filter selecting no tasks fail.

`--only` does **not** automatically include prerequisites. A filtered-out
blocking prerequisite warns and is assumed satisfied; a filtered-out
ordering-only predecessor does not warn. To include both kinds recursively:

```bash
dotfiles install --root . --profile desktop --no-repo-update \
  --only systemd --with-deps --dry-run
```

This can broaden the run to packages, AUR setup, links, and permissions: inspect
the graph first. `--skip` is applied after dependency expansion and can remove
a prerequisite again. Task names are not package names: `--only packages`
selects the whole active package list, not one package.

### Unattended runs

Pass `--profile`, `--root`, `--non-interactive`, and `--fail-on-skip` explicitly
when automation requires a complete result. A present `CI` variable, even
`CI=false`, enables strict skip handling and non-interactive policy.
Non-terminal stdin also enables non-interactive policy. Some tasks are
deliberately inapplicable in CI; strict mode does not turn those into failures.

Only one task-engine command can run per repository at a time, including
`check` and previews. Linked worktrees share a lock in the common Git directory.
`tasks` and `log` are exempt.

## Discover tasks

```bash
dotfiles tasks --root . --profile desktop --format json
dotfiles tasks --root . --profile desktop --graph install
dotfiles tasks --root . --profile desktop --graph install --only symlinks --with-deps
```

The normal list contains `SELECTOR`, `TASK`, and `COMMANDS`. It combines the
install, update, uninstall, check, and active overlay task sets. Listing a task
does **not** mean it applies on this host.

Graph output includes internal nodes. `BLOCKING` predecessors must succeed;
`AFTER` predecessors provide ordering only. `SELECTION` distinguishes default,
requested, dependency, filtered, and skipped nodes. Filters annotate selection
without hiding nodes or their edges. `--format plain` is tab-separated without
headings; `--format json` provides structured records.

Discovery still needs loadable configuration and a profile. If CLI,
environment, and saved profile selection are all absent, it requests
`--profile` instead of opening the profile prompt. An explicit linked-worktree
overlay retains its separate confirmation requirement.

## Update

To preview APM pin advancement from the current checkout:

```bash
dotfiles update --root . --profile desktop --no-repo-update --only apm --dry-run --verbose
```

Remove `--only apm` for the full install/update pipeline; use `--with-deps` if
the selected APM task needs its prerequisites converged. Remove
`--no-repo-update` only if you also want repository synchronization.
See [APM](APM.md) for update eligibility and generated manifest/lock behavior.

## Console output

The header identifies command, dry-run mode, profile, platform, and overlay.
Check it before interpreting a result. Rows appear as tasks finish, not in
configuration order.

| Status | Meaning |
|---|---|
| `✓` | Changes applied, or a validation check passed |
| `~` | Changes planned by a dry run |
| `○` | Already current; shown with `--verbose` |
| `⊘` | Skipped, blocked, or interrupted; read the accompanying reason |
| `✗` | Failed |

`--no-symbols` uses words, including distinct `SKIPPED`, `BLOCKED`, and
`INTERRUPTED` states. Non-applicable tasks are absent from console totals.
Internal tasks remain in logs but not normal rows or totals.

Indented lines describe completed or planned actions. `--verbose` adds current
tasks, decisions, and elapsed times. A transient `Running` line shows remaining
visible work and active tasks. Final counts summarize outcomes, for example
`2 changed · 14 current · 1 skipped · 2.3s`. **A zero exit status with skips is
not necessarily a complete installation.**

Failed-command summaries prefer concrete error diagnostics over update notices
and progress text, falling back to the first nonempty output line. The retained
log contains the captured output; the short row is not the whole diagnosis.

## Uninstall

Preview with the same profile and overlay used to install:

```bash
dotfiles uninstall --root . --profile desktop --dry-run --verbose
```

Removing `--dry-run` materializes selected managed home links, removes hooks
and the launcher still matching their managed state, and runs active overlay
scripts' `--remove` actions when their checks report managed state.
Modified/replaced hooks, unrelated hook names, and modified launchers are
preserved.

**Uninstall is not rollback.** It copies current configured sources, not
pre-install backups; even a missing home target can be materialized. Existing
non-link targets are preserved. It does not restore packages, Git/agent
settings, PATH, registry, services, permissions, shell selection, WSL, editor
extensions, or APM state. See [Uninstall tasks](TASKS.md#uninstall-tasks).
Keep the checkout and sources available until materialization succeeds.

## Check

`check` validates configuration and sources and runs available ShellCheck,
PSScriptAnalyzer, and local APM package checks. It does not install or repair
configuration and has no `--dry-run` option. Like other engine commands it
writes a log, locks the repository, and resolves/persists selections.

Configuration warnings fail `config-warnings`; missing analyzer executables
are visible skips, made fatal by `--fail-on-skip` or CI. A present `pwsh` with a
missing PSScriptAnalyzer module fails rather than skips.
Use [Testing — CLI validation](TESTING.md#cli-validation) for canonical
commands and coverage, and [Validation tasks](TASKS.md#validation-tasks) for
selectors. Narrowing tasks does not bypass initial configuration loading.

## Logs

Each install/update/uninstall/check process has a separate run log; the newest
50 are retained. Failed runs print an exact `dotfiles log --id ... -v` command
when persistent logging is healthy. Prefer that ID: numeric indexes shift
after subsequent runs.

```bash
dotfiles log                      # newest run
dotfiles log --list               # history with IDs, outcomes, durations, profiles
dotfiles log --list --format json
dotfiles log 2                    # third-newest run (zero-based)
dotfiles log --command install 1  # second-newest install run
dotfiles log --task symlinks      # one selector within the newest run
dotfiles log --raw                # original stored records
```

Use the actual ID from history with `--id`; it can be combined with `--task`
and either `--verbose` or `--raw`. It cannot be combined with an index,
`--list`, or `--command`. `--verbose` and `--raw` are mutually exclusive.
Command filters accept `install`, `update`, `uninstall`, and `check`; both
`update` and `install --update` produce update logs. The check filter also
recognizes legacy `test` logs.

Failed stdout/stderr and successful stderr are visible by default. Other
diagnostics require `--verbose`. Successful stdout is normally retained only
as a byte count unless a command requests full capture; `--raw` cannot recover
uncaptured output. Sanitize paths, private configuration, and tool output
before sharing logs.

History orders by recorded start time and links restarted/elevated children to
their parent IDs. `unfinished` means no finish record exists: a run may still
be active or may have terminated abruptly. Old logs without lifecycle records
show `unknown`; malformed or unfamiliar records remain readable as raw text.

Log-directory precedence is:

1. `DOTFILES_LOG_DIR`, used as supplied.
2. `LOCALAPPDATA` plus `dotfiles/logs`.
3. `XDG_STATE_HOME` plus `dotfiles/logs`.
4. The home directory plus `.local/state/dotfiles/logs`.
5. `./dotfiles/logs`.

Files use `<utc-timestamp>-<command>-<pid>.log`. Legacy cache-directory logs are
removed on the next run. Viewing logs does not create another log.

## Shell completions

The `completions` task installs runtime registration for Zsh or PowerShell.
The shell queries the current binary for profiles, log command values, and
task selectors. Task completion carries forward `--root`, `--profile`, and
`--overlay` and uses the same read-only configuration discovery as `tasks`.

## Repository and overlay paths

Use `--root .` from the intended repository root, or pass an absolute path.
Without it, the CLI tries `DOTFILES_ROOT`, candidate locations relative to its
executable, then the current directory. The current directory is **not**
preferred over the executable's checkout. Wrappers set `DOTFILES_ROOT` to
their own checkout.

`--root` selects configuration, not the executable: an installed launcher may
still obtain its binary from another checkout. Use that checkout's wrapper or
built binary when testing its source.

Overlay precedence is `--overlay`, `DOTFILES_OVERLAY`, then repository-local
Git config `dotfiles.overlay`. An explicit CLI overlay is persisted by engine
commands, **including previews and check**; `tasks` does not persist it.
Relative paths are made absolute relative to the invocation directory.

An explicit overlay whose `.git` is a file is treated as a linked worktree and
requires a separate `[y/N]` confirmation. No interactive terminal means that
selection is rejected. Use a stable primary checkout for unattended work.
See [Overlays](CONFIGURATION.md#overlays) for configuration semantics and
script trust boundaries.

## Interrupting a run

1. First Ctrl-C requests cancellation, stops dispatching new work, and waits
   for in-flight operations. Completed changes and plans remain in the summary.
2. Second Ctrl-C asks `Force quit? in-flight operations will be abandoned [y/N]:`.
   Anything except `y`/`yes` keeps waiting.
3. Confirming, or pressing Ctrl-C while the question is open, exits immediately
   with code 130. Without terminal interaction, the second Ctrl-C force quits.

Cancellation does not roll back changes. Force quitting skips orderly child
shutdown; external commands may remain active. Check them before retrying.
After a graceful stop, fix the cause and rerun to converge remaining state.

## Exit behavior

| Exit | Meaning |
|---|---|
| `0` | Command succeeded under its skip policy; inspect skipped work |
| `1` | Configuration/runtime error, task failure, or unmet work under strict policy |
| `2` | Command-line parsing/usage error |
| `130` | Interrupted; a genuine task failure takes precedence during graceful shutdown |

For symptom-driven recovery, start with [Troubleshooting](TROUBLESHOOTING.md).
