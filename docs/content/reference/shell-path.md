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

Runs after the launcher succeeds. On Linux, it inserts a managed export at the start of
`~/.profile`; on Windows, it updates the user `PATH`. Open a new shell to pick up the
change, since already-running processes keep the environment they inherited at startup.

Existing `PATH` entries stay intact, and `dotfiles remove` leaves this addition in place.
On Linux, an existing working `~/.profile` symlink is followed and its target file is
updated atomically with preserved permissions. Dangling links, non-regular files, and
files with multiple hard links are refused until you resolve them.

On Linux, Dotfiles manages `# Added by dotfiles` blocks in `~/.profile`:

- An empty profile gets a single `# Added by dotfiles` block; place your own commands
  **after** it.
- On the next update, Dotfiles adds a trailing block so a command that resets `PATH`
  cannot hide the launcher. Both blocks add `~/.local/bin` only if it is missing.
- When both blocks exist, keep shell commands between them; blank lines and comments
  can sit outside.
- If your profile replaces `PATH` and returns early, include `~/.local/bin` in that
  assignment.

The [Bash invocation reference](https://www.gnu.org/software/bash/manual/html_node/Invoking-Bash.html)
explains login and interactive startup modes, and
[Application settings](../guides/application-settings.md#shell-configuration) covers
this repository's shell startup files.

## Configuration

There is no TOML declaration; this task only ensures the launcher directory is on the
current user's `PATH`. Put application-specific `PATH` additions in your
[shell configuration](../guides/application-settings.md#shell-configuration).

## Preview and apply

Preview the persisted launcher-directory addition:

```sh
dotfiles update --only path --dry-run
```

Confirm that the addition is your home directory's `.local/bin`, not the checkout or
its `bin/` folder, and resolve any profile or saved-`PATH` read error before
[applying the change](task-reference.md#preview-and-apply). Open a fresh shell to
verify `dotfiles` is found; rerunning this task will not update an already-open
terminal.

If the update reports a conflict, it leaves `~/.profile` untouched. Move displaced
commands back inside the two blocks (or after the block if you have only one).
Dotfiles can migrate its exact legacy marked export at the end of a profile, but it
will not remove bare exports, rewrite CRLF shell commands, or guess whether a line in
an example is live code. Remove only unguarded additions you recognize, or skip the
`path` task if you prefer to manage `PATH` yourself.

If a fresh shell cannot find `dotfiles`, invoke the installed launcher directly with a
minimal `PATH` after fixing `~/.profile`:

```sh
PATH=/usr/bin:/bin "$HOME/.local/bin/dotfiles" update --only path --no-repo-update
```

The same launcher and temporary `PATH` work with the `log --id <run-id> -v` arguments
printed after a failure.

## Removal

The `PATH` addition stays in place after the launcher is removed. To retire it, edit
the managed export in `~/.profile` on Linux or the user `PATH` on Windows, keeping
unrelated entries intact, and open a new shell afterward.
