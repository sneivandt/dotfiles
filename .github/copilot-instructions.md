# GitHub Copilot Project Instructions

These guidelines help AI code assistants produce consistent, safe contributions to this dotfiles project.

## Project Overview
This project manages dotfiles and system configuration using a layered environment approach. It supports both Linux (specifically Arch Linux) and Windows.
- **Layered Configuration**: Configuration is split into "environments" (e.g., `base`, `arch`, `arch-gui`, `win`) located in the `env/` directory.
- **Idempotency**: All scripts are designed to be idempotent. Re-running the installation should simply verify the state without side effects or errors.
- **Cross-Platform**: The project uses `dotfiles.sh` (POSIX sh) for Linux and `dotfiles.ps1` (PowerShell) for Windows.
- **Goals**:
  - Provide reproducible, layered environment setup.
  - Keep scripts POSIX `/bin/sh` compatible.
  - Favor clarity over brevity; explicit checks and logging are preferred.

## Environment Structure (`env/`)
The `env/` directory contains the configuration layers. Each subdirectory (e.g., `env/base/`) represents a layer and may contain:
- `symlinks.conf`: A list of files to be symlinked.
- `packages.conf`: A list of system packages to install (e.g., via pacman on Arch).
- `units.conf`: Systemd user units to enable and start.
- `chmod.conf`: File permissions to apply to specific files.
- `submodules.conf`: Git submodules specific to that layer.
- `vscode-extensions.conf`: VS Code extensions to install.
- `symlinks/`: A directory containing the actual files to be linked.

## Symlink Management
Symlinks are managed declaratively.
- **Configuration**: Each `env/<layer>/symlinks.conf` lists the relative paths of files to link.
- **Source**: The source file is located at `env/<layer>/symlinks/<path>`.
- **Target**: The target is always relative to the user's home directory, prefixed with a dot.
  - Example: A line `config/nvim` in `symlinks.conf` maps `env/base/symlinks/config/nvim` to `~/.config/nvim`.
- **Rule**: Do not hardcode `ln -s` commands in scripts. Always add the file to the appropriate `symlinks/` folder and update `symlinks.conf`.
- **Backups**: Do not backup existing files before linking. This is not necessary.

## Shell Scripting
- Use `#!/bin/sh` unless there is a compelling reason for Bash. If Bash required, document it.
- Always start new shell scripts with:
  ```sh
  #!/bin/sh
  set -o errexit
  set -o nounset
  ```
- Use double quotes around variable expansions except when intentional word splitting is required (add a shellcheck directive comment there).
- Avoid process substitution and arrays (Bash features) in POSIX scripts.
- Logging: use the existing `log_stage`, `log_error` helpers instead of ad‑hoc echo statements for operational messages.
- Guard optional external tool usage with `is_program_installed`.
- For loops over environment layers should reuse the pattern: `for env in "$DIR"/env/*; do ...; done` and respect `is_env_ignored`.
- Avoid unnecessary subshells unless isolating environment changes.
- Prefer constructing minimal lists before calling system package managers.
- Always quote glob patterns when iterating variable-expanded paths.

## PowerShell
- Match existing style: Verb-Noun function names, comment-based help, export only necessary functions via `Export-ModuleMember`.
- Windows automation should fail gracefully when run without elevation if elevation is required.

## Testing & CI
- Perform static analysis by running `dotfiles.sh -T` which includes shellcheck and other linters.
- Ensure all scripts are idempotent; re-running should not cause errors or unintended changes.