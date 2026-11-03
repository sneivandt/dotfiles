---
title: Application settings
description: Find the source for a common application change, understand when edits take effect, and customize Git and shared shell preferences.
---

# Application settings

Application preferences live in tracked files under
[symlinks/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/).
Edit those sources, not installed copies or generated files. A home-directory
link may make this checkout live configuration, so some changes take effect
immediately.

Installing an application and configuring it are separate changes.
[Package management](package-management.md) chooses applications;
[symlink declarations](configuration-reference.md#symlinks) choose which
settings files reach your home. The files themselves control application
behavior. This guide helps you find the right source, then covers Git and
preferences shared by the shells.

## Find a common change

| I want to change... | Start here |
|---|---|
| Git identity, defaults, aliases, or templates | [Git settings](#git) below |
| Bash/Zsh environment or shared commands | [Shared shell sources](#shell-application-configuration) below |
| User command directories on PATH | [config/shell/path.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/path.sh) for Bash/Zsh, or the [PowerShell profile](windows-and-wsl.md#powershell-configuration) |
| Bash or Zsh prompt | [config/bash/prompt.bash](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/bash/prompt.bash) or [config/zsh/prompt.zsh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/prompt.zsh) |
| PowerShell behavior | [PowerShell configuration](windows-and-wsl.md#powershell-configuration) |
| Terminal colors, fonts, or keys | [Alacritty settings](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/alacritty/alacritty.toml) or [Windows Terminal settings](https://github.com/sneivandt/dotfiles/blob/main/symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json) |
| tmux behavior | [config/tmux/tmux.conf](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/tmux/tmux.conf) |
| Desktop shortcuts, monitors, bar, or appearance | [Arch desktop](arch-desktop.md#where-to-customize) |
| Editor settings, keybindings, or plugins | [Visual Studio Code](visual-studio-code.md) or [Vim and Neovim](vim-and-neovim.md) |
| Agent preferences, skills, or integrations | [Agent configuration](agent-configuration.md) |
| SSH client defaults | [ssh/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/ssh/config) |
| Automatic deletion of old temporary files or screenshots | [Automatic cleanup](#automatic-cleanup) below |

Source paths in this table are beneath `symlinks/`. Linked source files open
the public repository on GitHub; edit the corresponding file in your own
checkout. Paths such as `~/.config/git/config` describe installed locations,
not a second source to maintain.

## Make the change

For a preference in an already-linked file, edit the source and use the
application's normal reload or restart behavior. You do not need to recreate
the link. Some settings UIs, including VS Code's, can write back through the
link; review `git diff` afterward.

For a new file or destination, update
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml),
then preview the link task before applying. **Back up existing targets:** installing a managed
link can replace a file without making a backup. The
[first-run example](getting-started.md#2-review-the-configuration) shows the
declaration-to-source-to-home relationship.

Use a [private overlay](private-overlays.md) for machine-specific alternatives.
An overlay can replace a linked source, but does not merge arbitrary application
file contents. Keep credentials and private identity details out of public
sources.

## Git

Git identity and defaults live in
[config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config);
command shortcuts live in
[config/git/aliases](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases).
Review the supplied identity before using it. These shared settings affect Git
beyond this repository; keep private identity details in a
[private overlay](private-overlays.md).

Desired global key/value overrides belong in
[conf/git-config.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml).
The [Git settings format](configuration-reference.md#git-settings) explains
platform selection and Windows autocrlf cleanup. Avoid contradicting the linked
files with a second declaration.

| Shortcut | Purpose |
|---|---|
| `git st` | Show concise status and branch information |
| `git dis` | Review staged changes |
| `git lo` | Show a compact commit graph |
| `git gr <pattern>` | Search tracked content |

Run `git alias` to list all aliases. If a setting does not take effect, use
`git config --show-origin --get <key>` in the affected repository, replacing
`<key>` with the setting name. Included files and repository settings can
override the shared configuration.

### Git templates

The shared configuration selects
[Git templates](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git/templates/)
through `init.templateDir`. Git copies their hook entrypoints when initializing
or cloning a repository; the helpers resolve from the managed directory at
runtime. These are separate from this repository's [contributor hooks](git-hooks.md).
Existing repositories need new entrypoints installed explicitly without
overwriting repository-specific hooks.

**The templates can automatically delete local branches.** After a merge or
rebase, [branch cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/templates/hooks.local/clean-merged-branches)
deletes eligible branches merged into `origin/HEAD` using `git branch -d`.
It preserves the current and default branches, `main`, `master`, `dev`,
`develop`, and branches used by any worktree. It never deletes remote branches,
does nothing without `origin/HEAD`, and does not run after an amend.

The [push-protection hook](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/templates/hooks.local/pre-push)
blocks updates to `main`, `master`, `develop`, and the destination remote's
default branch. Add exact names through repository-local
`dotfiles.protectedBranch` values. GitHub remotes whose URL owner matches
`github.user` or its fallback `github.username` are exempt; that is a local
configured-name check, not a GitHub permission check.

## Shell application configuration

| Setting | Source |
|---|---|
| Bash and Zsh interactive startup | [bashrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bashrc), [zshrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zshrc) |
| Bash and Zsh login profiles | [bash_profile](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bash_profile), [zprofile](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zprofile) |
| Shared shell environment and pager defaults | [config/shell/profile.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/profile.sh) |
| Shared commands and aliases | [config/shell/aliases.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/aliases.sh) |
| Zsh completion | [config/zsh/completion.zsh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/completion.zsh) |
| Zsh command helpers | [config/zsh/functions/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/zsh/functions/) |
| Download naming and timestamping | [config/wgetrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/wgetrc) |

### Environment and paging

The login profiles source shared environment settings; interactive startup
files source shared aliases when present. Put
behavior common to Bash and Zsh in `config/shell/`; keep shell-specific
behavior in its own startup file. The [Windows guide](windows-and-wsl.md#powershell-configuration)
covers PowerShell's separate profile.

The shared profile supplies defaults for XDG directories, the editor, locale,
terminal information, and the `less` pager. It preserves existing values for
settings such as `EDITOR`, `VISUAL`, `LANG`, and the XDG directories. Other
preferences, including pager colors and `WGETRC`, are assigned on startup;
change those in `config/shell/profile.sh` rather than expecting an environment
override to win.

`config/shell/path.sh` puts user command directories ahead of inherited PATH
entries and removes duplicates of those entries. It honors `CARGO_HOME`,
`GOBIN`, and `GOPATH`. This shell startup behavior is separate from the CLI's
[PATH task](task-reference.md#shell-path), which makes the dotfiles launcher
available persistently.

### Completion

Zsh uses normal completion on the first Tab. Repeat Tab to try approximate
matches.

Dotfiles command completion comes from the CLI's
[Shell completions task](task-reference.md#shell-completions), not a separate
hand-edited selector list.

### Downloads and archives

Wget preserves URL filenames and uses timestamping to reuse unchanged
downloads. Change its shared defaults in `config/wgetrc`.

In Zsh, the [extract helper](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/functions/extract)
accepts `extract <file>` and chooses the tool for the archive format. That tool
must be installed;
RPM extraction uses `bsdtar`, supplied by `libarchive` on Arch.
Keep the extractor's normal path protections when customizing the helper.
Extract untrusted archives into a fresh directory and review the contents
before running anything; see [Security and trust](security-and-trust.md).

## Automatic cleanup

The `systemd` task enables user timers that **delete old content automatically**.
Review them before using these directories for anything you intend to keep:

| Selected environment | Directory | File and symlink retention rule |
|---|---|---|
| Linux, either profile | `~/tmp` | `find -mtime +7`: at least 8 full days since modification |
| Arch desktop | `~/screenshots` | `find -mtime +30`: at least 31 full days since modification |

The timers run daily, with up to ten minutes of randomized delay. They catch
up after a missed run, so cleanup can happen soon after the user manager
starts. The services skip absent directories, stay on the same filesystem,
do not follow symlinks, and remove old empty subdirectories too. Deletion does
not use the desktop trash or make a backup.

Retention commands live in
[clean-home-tmp.service](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service)
and
[clean-screenshots.service](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-screenshots.service);
their adjacent `.timer` files control scheduling. Inspect the active timers
without running cleanup:

```sh
systemctl --user list-timers --all --no-pager
```

To disable cleanup persistently through dotfiles, change the relevant unit in
`conf/systemd-units.toml` to an explicit
`{ name = "clean-home-tmp.timer", scope = "user", enabled = false }` entry
(or use `clean-screenshots.timer`). Review and apply the `systemd` task.
Simply deleting the declaration or running `dotfiles remove` does not stop a
timer that was already enabled.
