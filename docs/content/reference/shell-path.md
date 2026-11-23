---
title: Shell PATH
description: Add the dotfiles launcher directory to the user PATH on Linux or Windows and distinguish command discovery from launcher installation.
---

# Shell PATH

PATH is the list of directories a shell searches for commands. This task
persists the directory containing the [Dotfiles launcher](dotfiles-launcher.md)
so a future shell can find it. It does not install commands, choose the login
shell, or repair every application's PATH configuration.

| Task detail | Value |
|---|---|
| Selector | `path` |
| Commands | `update` |
| Availability | Linux or Windows |
| [Requires success](task-reference.md#scheduling-model) | [Dotfiles launcher](dotfiles-launcher.md) |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [dotfiles/path.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/path.rs) |

## What it does

Requires the launcher. On Linux, the managed export runs at the start of
`~/.profile`. On Windows, the task updates the user PATH. Open a new shell to
use the persisted change.

Existing PATH entries are preserved. Remove leaves the PATH addition in place.
On Linux, an existing working `~/.profile` symlink is preserved and its destination
is updated. Dangling links, non-regular files, and files with multiple hard links
are refused; resolve those destinations before retrying. Updates preserve existing
file permissions and replace the content atomically.

Open a new terminal to use the updated PATH. Existing processes keep the
environment they inherited at startup, and on Linux loading also depends on the
shell's startup files.

On Linux, Dotfiles manages `# Added by dotfiles` blocks in `~/.profile`:

- An empty profile gets one `# Added by dotfiles` block; put new commands
  **after** it.
- On the next update, Dotfiles adds a trailing block so a command that resets
  PATH does not hide the launcher. Each block adds `~/.local/bin` only if it is
  absent.
- When two blocks are present, keep shell commands between them; blank lines
  and comments can stay outside.
- If your profile replaces PATH and then returns early, include `~/.local/bin`
  in that assignment.

The [Bash invocation reference](https://www.gnu.org/software/bash/manual/html_node/Invoking-Bash.html)
explains login and interactive shell modes; the managed
[shell configuration](../guides/application-settings.md#shell-configuration)
supplies the repository-specific startup behavior.

## Configuration

There is no TOML declaration. The task adds the launcher directory for the current user.
Application-specific PATH additions belong in your [shell
configuration](../guides/application-settings.md#shell-configuration).

## Preview and apply

Preview the persisted launcher-directory addition:

```sh
dotfiles update --only path --dry-run
```

The intended addition is your home directory's `.local/bin`, not the checkout
or its `bin/` directory. Resolve any profile or saved-PATH read error before
[applying the change](task-reference.md#preview-and-apply). Open a new shell and
confirm it finds the launcher; retrying this task does not refresh an existing
terminal's environment.

If the update reports a conflict, it has not rewritten the profile. Move
displaced commands back inside the two blocks, or after the block if you
have only one. Dotfiles can migrate the exact old marked export at the
end of a profile. It does not remove a bare export, fix CRLF shell commands,
or guess which copy in an example is live shell code. Remove only an
unguarded addition you recognize; keep literal examples intact. If you
prefer to manage PATH yourself, skip the `path` task instead.

If a fresh shell cannot find `dotfiles`, use the installed launcher directly
with a temporary PATH for its standard utilities. After fixing the profile, run:

```sh
PATH=/usr/bin:/bin "$HOME/.local/bin/dotfiles" update --only path --no-repo-update
```

The same launcher and temporary PATH work with the `log --id <run-id> -v`
arguments printed after a failure.

## Removal

The PATH addition remains after the launcher is removed. To retire it, edit the managed
export in `~/.profile` on Linux or the user PATH on Windows. Preserve unrelated entries
and open a new shell afterward.
