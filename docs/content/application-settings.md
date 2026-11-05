---
title: Application settings
description: Find the source for a common application change, understand when edits take effect, and customize Git, shells, and editors.
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
behavior. This guide helps you find the right source, then covers Git, shells,
and editors.

## Find a common change

| Change | Start here |
|---|---|
| Git identity, defaults, aliases, or templates | [Git settings](#git) below |
| Bash/Zsh environment or shared commands | [Shell configuration](#shell-configuration) below |
| User command directories on PATH | [config/shell/path.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/path.sh) for Bash/Zsh, or the [PowerShell profile](windows-and-wsl.md#powershell-configuration) |
| Bash or Zsh prompt | [config/bash/prompt.bash](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/bash/prompt.bash) or [config/zsh/prompt.zsh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/prompt.zsh) |
| PowerShell behavior | [PowerShell configuration](windows-and-wsl.md#powershell-configuration) |
| Terminal colors, fonts, or keys | [Alacritty settings](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/alacritty/alacritty.toml) or [Windows Terminal settings](https://github.com/sneivandt/dotfiles/blob/main/symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json) |
| tmux behavior | [config/tmux/tmux.conf](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/tmux/tmux.conf) |
| Desktop shortcuts, monitors, bar, or appearance | [Arch desktop](arch-desktop.md#where-to-customize) |
| Editor settings, keybindings, or plugins | [Visual Studio Code](#visual-studio-code) or [Vim and Neovim](#vim-and-neovim) |
| Agent preferences, skills, or integrations | [Agent configuration](agent-configuration.md) |
| SSH client defaults | [ssh/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/ssh/config) |

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
then preview the link task:

```sh
dotfiles update --only symlinks --dry-run
```

This previews all active links, including your new entry. **Back up existing
targets:** installing a managed link can replace a file without making a backup.
Remove `--dry-run` only after reviewing the result. The
[first-run example](getting-started.md#2-review-the-configuration) shows how a
declaration connects a source file to its installed location.

Use a [private overlay](private-overlays.md) for machine-specific alternatives.
An overlay can replace a linked source, but does not merge arbitrary application
file contents. Keep credentials and private identity details out of public
sources.

### Back up and restore a file

Before the first apply, preserve any existing file at a destination you plan
to manage. For the Git configuration on Linux, run this only if that file
exists and is readable:

```sh
backup_dir=$(mktemp -d "$HOME/dotfiles-backup.XXXXXX")
cp -pL "$HOME/.config/git/config" "$backup_dir/config"
printf 'Backup: %s\n' "$backup_dir/config"
```

Open the backup and confirm its contents before applying. `cp -L` saves the
file's contents even if the existing destination is a symlink. For a symlink,
also record its target with `readlink`; the backup above preserves content,
not the original link. Back up other selected files separately, including any
Git configuration included by this file. On Windows, copy the resolved file's
contents to a separate backup directory and record any original link target.

To restore, first stop selecting the managed link in your declarations. Move
the current destination itself aside, then copy the backup into its place as
a real file. Copying directly over a managed symlink could overwrite the
checkout's source. If you want the original symlink back instead, recreate its
recorded target only after confirming that target still exists. Check
`git config --show-origin --get user.name` to see which file Git now reads.

## Git

Git identity and defaults live in
[config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config);
command shortcuts live in
[config/git/aliases](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases).
Review the supplied identity before using it. The shared configuration also
rewrites GitHub HTTPS push URLs to SSH, so pushes need your own SSH
authentication. These settings affect Git beyond this repository; use the
[overlay file example](private-overlays.md#replace-an-application-file) for a
private identity or different defaults.

To have dotfiles apply individual global Git settings, declare them in
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
runtime. These are separate from this repository's [contributor hooks](contributing.md#git-hooks).
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

## Shell configuration

In Zsh, `extract <file>` keeps the original archive. Use
`extract --remove <file>` to delete it after successful extraction.

| Setting | Source |
|---|---|
| Bash and Zsh interactive startup | [bashrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bashrc), [zshrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zshrc) |
| Bash and Zsh login profiles | [bash_profile](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bash_profile), [zprofile](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zprofile) |
| Shared shell environment and pager defaults | [config/shell/profile.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/profile.sh) |
| Shared commands and aliases | [config/shell/aliases.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/aliases.sh) |
| Zsh completion | [config/zsh/completion.zsh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/completion.zsh) |
| Zsh command helpers | [config/zsh/functions/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/zsh/functions/) |

### Environment and paging

Login shells load the shared environment settings; interactive shells load
the shared aliases. Put behavior common to Bash and Zsh in `config/shell/`,
and shell-specific behavior in that shell's startup file. The
[Windows guide](windows-and-wsl.md#powershell-configuration) covers PowerShell's
separate profile.

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

## Visual Studio Code

The desktop profile installs VS Code Insiders on Arch and Windows. It links
shared settings into both stable and Insiders locations, even if only one
edition is installed. Edit
[settings.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/Code/User/settings.json)
and [keybindings.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/Code/User/keybindings.json).
The settings choose Tokyo Night, Source Code Pro, word wrapping, save on focus
change, and Zsh on Linux or PowerShell on Windows. Application updates are manual;
extension updates are separate. Review the checkout diff after using the settings
UI, which can write through managed links.

The desktop profile links these locations beneath the home directory:

- Linux: `.config/Code/User/` and `.config/Code - Insiders/User/`
- Windows: `AppData/Roaming/Code/User/` and `AppData/Roaming/Code - Insiders/User/`
- Remote servers: `.vscode-server/data/Machine/settings.json` and
  `.vscode-server-insiders/data/Machine/settings.json`

Remote server locations receive settings only. Run dotfiles in the home that
owns them; configuring Windows does not configure WSL or another host. Workspace
settings can override user settings. The `base` profile does not select these links.

### Extensions

Edit [conf/vscode-extensions.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/vscode-extensions.toml)
using Marketplace IDs in the appropriate profile category. See
[Extension declarations](configuration-reference.md#vs-code-extensions) for syntax,
or use an overlay for personal additions. On a configured machine, preview with:

```sh
dotfiles update --only vscode-extensions --dry-run
```

Remove `--dry-run` after review. Use a normal update for a first installation so
the application and prerequisites are included. Installation prefers
`code-insiders`, otherwise `code` (their `.cmd` launchers on Windows). It installs
missing IDs, comparing names without case sensitivity. An unreadable installed
list fails the task; no available CLI means a skip.

The list is neither a version lockfile nor an uninstall list. Existing extensions
remain, including after removing declarations or running `dotfiles remove`.
Use the editor's Extensions view for updates and deliberate removal.

On Arch, the managed Insiders launcher and URL handler use GNOME libsecret.
See [GNOME Keyring troubleshooting](troubleshooting.md#gnome-keyring-stays-locked)
for credential failures. Chroot setup defers extension installation until a
graphical login; `dotfiles-first-login.service` retries until it succeeds.

For failures, start with `dotfiles log --task vscode-extensions --verbose`.
Check the selected CLI, profile, home, link destinations, and workspace overrides
before reinstalling anything. Agent integrations have separate
[configuration](agent-configuration.md).

## Vim and Neovim

On Linux, `symlinks/vim/` links to both `~/.vim` and `~/.config/nvim`, including
for the base profile. Vim uses the shared configuration without third-party
plugins; Neovim adds lazy.nvim plugins. Edits and plugin lockfile updates can
therefore modify tracked files in this checkout.

| Source | Change here |
|---|---|
| [vimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/vimrc) | Shared behavior and mappings |
| [init.vim](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/init.vim) | Neovim entry point |
| [nvimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/nvimrc) | Neovim mappings and terminal behavior |
| [lazy-bootstrap.lua](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/lua/lazy-bootstrap.lua) | Plugin declarations, configuration, and bootstrap pin |
| [lazy-lock.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/lazy-lock.json) | Recorded plugin revisions |

The leader key is comma. Shared shortcuts include `,w` for wrapping, `,k` for
spell checking, and `,v` for configuration. Neovim adds Ctrl-P for file search,
`,/` for content search with ripgrep, Ctrl-E for the tree, `,b` for buffers,
and `,t` for a terminal. See the source mappings for the full set, including
remapped navigation and search keys. In a terminal, Escape leaves input mode;
Alt+Escape also returns to the previous window.

### First launch and plugins

Starting Neovim can clone the pinned lazy.nvim manager and install missing
plugins. It needs Git and network access and is **not a read-only check**.
Use `:echo stdpath('data')` to locate its data directory; the manager lives
under `lazy/lazy.nvim` there. Existing installations are not reset on startup.

Run `:TSInstallConfigured` separately for missing configured Tree-sitter parsers.
Wait for the asynchronous installation, then reopen the file. Normal startup
does not download parsers; the plugin's `:TSUpdate` build hook updates installed
parsers during plugin updates.

Use `:Lazy` to inspect plugin state and errors. `:Lazy update` updates revisions;
`:Lazy sync` also installs missing plugins and cleans unused ones. These commands
change installed content and may change the tracked lockfile, so review the diff.
Keep plugin-only behavior on the Neovim path so plain Vim still loads on its own.
Neovim 0.11+ uses fuzzy insert completion; 0.12+ adds proximity ranking and `,u`
for the built-in undo tree.

For errors, inspect `:messages`, `:Lazy`, the failing Git command, and the affected
plugin's locked revision. Do not delete the entire plugin tree as a general
repair. If necessary, close the editor and move aside only the affected directory
under the actual data path before retrying.

Contributors changing bootstrap pins or plugin loading should use
[Editor configuration](https://github.com/sneivandt/dotfiles/blob/main/.agents/skills/editor-configuration/SKILL.md)
and the isolated [application configuration tests](testing.md#application-configuration).
Do not verify first-launch behavior through the live installed profile.
