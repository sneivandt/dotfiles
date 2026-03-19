---
name: overlay-scripts
description: >
  Overlay repository system and convention-based script tasks. Use when
  working with private overlay config, custom script resources, or the
  dynamic task injection mechanism.
metadata:
  author: sneivandt
  version: "1.0"
---

# Overlay Scripts

The overlay system allows a separate repository to extend the main dotfiles
configuration with private TOML config and custom script tasks.

## Overlay Path Resolution

Resolved in priority order (same pattern as profiles):

1. `--overlay` CLI flag
2. `DOTFILES_OVERLAY` environment variable
3. `dotfiles.overlay` in the repo's local git config

Implemented in `cli/src/config/overlay.rs` with `resolve_from_args()`.
Persisted via `git2` crate to local git config.

## Config Merging

`Config::load()` accepts an optional overlay path. When set, it reads
every standard TOML file from `<overlay>/conf/` (if present) and appends
entries to the main config lists. The same category filtering applies.

```rust
// In Config::load() — merge_overlay() handles overlay loading
if let Some(overlay_root) = overlay {
    let overlay_conf = overlay_root.join("conf");
    // Load each TOML type if the file exists, append to vectors
}
```

## Script Convention

Scripts in the overlay follow a three-mode interface:

| Invocation | Purpose | Expected behaviour |
|---|---|---|
| No args | **Apply** | Create/install the desired state |
| `--check` | **Check** | Exit 0 if correct, non-zero if needs apply |
| `--remove` | **Remove** | Undo the applied state |

### PowerShell scripts (`.ps1`)

```powershell
param([switch]$Check, [switch]$Remove)

if ($Check)  { <# verify state, exit 0/1 #> }
elseif ($Remove) { <# undo #> }
else { <# apply #> }
```

Invoked with: `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File`

### Shell scripts (`.sh`)

```sh
#!/bin/sh
case "$1" in
  --check)  # verify state, exit 0/1 ;;
  --remove) # undo ;;
  *)        # apply ;;
esac
```

Invoked with: `sh`

## Script Resource

`cli/src/resources/script.rs` — `ScriptResource` implements both
`Applicable` and `Resource`:

- `current_state()` — runs `--check`, maps exit code to `Correct`/`Missing`
- `apply()` — runs script with no args
- `remove()` — runs script with `--remove`
- Returns `Skipped`/`Invalid` when the script file is missing

The `interpreter_args()` method selects the interpreter based on file extension.

## Dynamic Task Injection

Unlike static tasks in the catalog, overlay scripts produce dynamic tasks:

1. `LoadOverlayScripts` is a static task in `all_install_tasks()` — runs in
   the Repository phase, validates overlay is configured and logs script count
2. `CommandRunner::overlay_script_tasks()` creates one `OverlayScriptTask`
   per `ScriptEntry` from the loaded config
3. `install.rs` extends the static task list with these dynamic tasks before
   filtering and execution
4. Each `OverlayScriptTask` runs in the Apply phase; the phase barrier
   guarantees `LoadOverlayScripts` (Repository) completes first

```rust
// In install.rs
let mut all_tasks = tasks::all_install_tasks();
all_tasks.extend(runner.overlay_script_tasks());
```

## Overlay Repository Structure

```
overlay-repo/
  conf/
    scripts.toml        # Script task definitions
    packages.toml       # Additional packages (optional)
    symlinks.toml       # Additional symlinks (optional)
    ...                 # Any standard conf/*.toml file
  scripts/
    config.ps1           # Convention-based script
    ssh.sh              # Another script
```

## `scripts.toml` Format

```toml
[linux]
scripts = [
  { name = "Setup work SSH", path = "scripts/ssh.sh" },
]
```

Parsed by `cli/src/config/scripts.rs` using the `config_section!` macro.
Paths are relative to the overlay root.

## Rules

- Scripts must handle all three modes: apply, `--check`, `--remove`
- `--check` must exit 0 when state is correct, non-zero otherwise
- PowerShell scripts run with `-NonInteractive` to prevent prompts
- Use `[System.IO.Directory]::Delete()` for directory symlinks in
  PowerShell (avoids `Remove-Item` confirmation prompts)
- Overlay TOML files are **appended** to main config, never replace
- The overlay path is resolved once at startup; changes require a re-run
- Dynamic tasks use the script entry's `name` field as the task name
