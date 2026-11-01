---
name: shell-configuration
description: >
  Use for managed Bash/Zsh startup files, symlinks/config/shell/, shell prompts,
  completion, history, PATH, and command helpers under symlinks/config/zsh/.
  Not for dotfiles.sh/dotfiles.ps1 bootstrap, Git hooks, or Rust shell tasks.
---

# Shell Configuration

## Find the owner

| Change | Start with |
|---|---|
| login startup | `symlinks/bash_profile`, `symlinks/zprofile` |
| shared environment and PATH | `symlinks/config/shell/profile.sh`, `path.sh` |
| interactive startup | `symlinks/bashrc`, `symlinks/zshrc` |
| completion, prompt, key bindings | `symlinks/config/zsh/`, `symlinks/config/bash/` |
| command helpers | `symlinks/config/zsh/functions/` |

Read the affected entry point and its source order before changing a helper.
Login, interactive, and noninteractive shells have different responsibilities;
do not add terminal queries or UI initialization to shared startup merely to
make an interactive case pass.

## Preserve startup and helper contracts

- Establish user command paths before capability checks. Preserve PATH ordering,
  literal entries containing spaces, and idempotency when startup is sourced again.
- Preserve explicit environment overrides and XDG paths. Keep shared `.sh`
  fragments compatible with both Bash and Zsh; native Zsh syntax belongs in its
  own files.
- Keep optional tools optional and startup usable when they are absent. Avoid
  adding subprocesses, network requests, or writes to every prompt/login when
  existing lazy initialization or caching can serve the same purpose.
- Preserve completion trust checks and cache behavior. Keep history selection
  distinct from accepting/executing the selected command.
- Pass external paths and values as quoted arguments. Preserve helper exit
  status, cleanup, and explicit confirmation around destructive actions.

## Verify in isolation

Start with the [shell regressions](../../../.github/workflows/scripts/linux/test-shell-config.py):
they provide temporary homes, a controlled environment, fake tools, and startup
fixtures. Cover login versus interactive behavior, absent tools, spaces,
user overrides, repeat sourcing, and failure paths relevant to the change.
Do not source the changed profile in the user's live shell to validate it.

Use [Application configuration](../../../docs/content/testing.md#application-configuration)
for commands and dependencies. Use `shell-patterns` for bootstrap wrappers,
`symlink-management` when link declarations change, and `profile-system` only
when profile resolution changes.
