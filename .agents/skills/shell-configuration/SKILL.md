---
name: shell-configuration
description: >
  Use for managed Bash/Zsh startup files, symlinks/config/shell/, shell prompts,
  completion, history, PATH, and command helpers under symlinks/config/zsh/.
  Not for dotfiles.sh/dotfiles.ps1 bootstrap (use self-update), Git hooks
  (use git-hooks-patterns), or Rust shell tasks.
---

# Shell Configuration

## Non-obvious invariants

- Keep login (`symlinks/bash_profile`, `symlinks/zprofile`), shared
  (`symlinks/config/shell/`), and interactive (`symlinks/bashrc`,
  `symlinks/zshrc`, `symlinks/config/{bash,zsh}/`) responsibilities separate;
  never put terminal queries or UI initialization in shared/noninteractive startup.
- Keep shared `.sh` fragments compatible with both Bash and Zsh and safe under
  `set -u` (`${VAR:-}` / `${VAR+x}`) without altering caller shell options;
  keep native Zsh syntax in Zsh-only files.
- Establish user command paths before capability checks, preserve explicit
  environment/XDG overrides, handle spaces in `PATH` entries, and keep repeated
  sourcing idempotent.
- Preserve completion trust checks (`compaudit`/ownership) and cache behavior,
  and keep history selection distinct from executing the selected command.

## Verification

Run [test-shell-config.py](../../../tests/shell/test-shell-config.py)
(isolated homes, controlled env, fake tools) rather than sourcing changes in the
live shell. See [Application configuration](../../../docs/content/development/testing.md#application-configuration).
