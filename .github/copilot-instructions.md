# GitHub Copilot Project Instructions

These guidelines help AI code assistants produce consistent, safe contributions to this dotfiles project.

## Project Goals
- Provide reproducible, layered environment setup across Linux (Arch focus) and Windows.
- Keep scripts POSIX `/bin/sh` compatible (avoid Bash-only constructs in `.sh`).
- Maintain idempotency: re-running install should not produce errors or duplicate work.
- Favor clarity over brevity; explicit checks and logging are preferred.

## Conventions
- Shell scripts: use `#!/bin/sh` unless there is a compelling reason for Bash. If Bash required, document it.
- Always start new shell scripts with:
  ```sh
  #!/bin/sh
  set -o errexit
  set -o nounset
  set -o pipefail
  ```
- Use double quotes around variable expansions except when intentional word splitting is required (add a shellcheck directive comment there).
- Avoid process substitution and arrays (Bash features) in POSIX scripts.
- Logging: use the existing `log_stage`, `log_error` helpers instead of ad‑hoc echo statements for operational messages.
- Guard optional external tool usage with `is_program_installed`.
- For loops over environment layers should reuse the pattern: `for env in "$DIR"/env/*; do ...; done` and respect `is_env_ignored`.

## Symlink Management
- Symlinks must remain declarative via `symlinks.conf` (or JSON variant if added in future). Do not hardcode specific user file paths directly in scripts.
- If introducing backup behavior, keep backups within `~/.dotfiles_backup/<timestamp>/`. Never silently overwrite user data.

## PowerShell
- Match existing style: Verb-Noun function names, comment-based help, export only necessary functions via `Export-ModuleMember`.
- Windows automation should fail gracefully when run without elevation if elevation is required.

## Testing & CI
- Preserve existing analyzer test hooks (`./dotfiles.sh -T`).
- When adding new linting or validation, ensure it is safe in minimal containers (no interactive prompts).

## Performance & Safety
- Avoid unnecessary subshells unless isolating environment changes.
- Prefer constructing minimal lists before calling system package managers.
- Always quote glob patterns when iterating variable-expanded paths.

## What NOT to Do
- Do not introduce Bashisms into existing `/bin/sh` scripts.
- Do not auto-install large toolchains unrelated to core dotfiles.
- Do not store secrets or machine-specific credentials.
- Do not assume non-Arch distros unless adding a generic abstraction layer.

## Prompt Engineering (For Copilot)
When asking Copilot for help inside this repo, include context like:
- Target: POSIX shell or PowerShell
- Desired idempotency (describe what happens if re-run)
- External tools allowed (e.g., pacman, systemctl)

Example prompt:
> Generate a POSIX sh function that ensures a list of directories exists, using log_stage only once if at least one directory had to be created.