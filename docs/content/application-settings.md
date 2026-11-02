---
title: Application settings
description: Find the source for a common application change, understand when edits take effect, and customize shared shell preferences.
---

# Application settings

Application preferences live in tracked files under
[`symlinks/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/).
Edit those sources, not installed copies or generated files. A home-directory
link may make this checkout live configuration, so some changes take effect
immediately.

Installing an application and configuring it are separate changes.
[Package management](package-management.md) chooses applications;
[symlink declarations](configuration-reference.md#symlinks) choose which
settings files reach your home. The files themselves control application
behavior. This guide helps you find the right source, then covers preferences
shared by the shells.

## Find a common change

| I want to change... | Start here |
|---|---|
| Git identity, defaults, or aliases | [`config/git/config`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config), [`config/git/aliases`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases), and [Git settings](configuration-reference.md#git-settings) |
| Bash/Zsh environment or shared commands | [Shared shell sources](#shell-application-configuration) below |
| Bash or Zsh prompt | [`config/bash/prompt.bash`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/bash/prompt.bash) or [`config/zsh/prompt.zsh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/prompt.zsh) |
| PowerShell behavior | [PowerShell configuration](windows-and-wsl.md#powershell-configuration) |
| Terminal colors, fonts, or keys | [Alacritty settings](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/alacritty/alacritty.toml) or [Windows Terminal settings](https://github.com/sneivandt/dotfiles/blob/main/symlinks/AppData/Local/Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json) |
| tmux behavior | [`config/tmux/tmux.conf`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/tmux/tmux.conf) |
| Desktop shortcuts, monitors, bar, or appearance | [Arch desktop](arch-desktop.md#where-to-customize) |
| Editor settings, keybindings, or plugins | [Visual Studio Code](visual-studio-code.md) or [Vim and Neovim](vim-and-neovim.md) |
| Agent preferences, skills, or integrations | [Agent configuration](agent-configuration.md) |
| SSH client defaults | [`ssh/config`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/ssh/config) |

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
[`conf/symlinks.toml`](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml),
then preview the link task before applying. **Back up existing targets:** installing a managed
link can replace a file without making a backup. The
[first-run example](getting-started.md#2-review-the-configuration) shows the
declaration-to-source-to-home relationship.

Use a [private overlay](private-overlays.md) for machine-specific alternatives.
An overlay can replace a linked source, but does not merge arbitrary application
file contents. Keep credentials and private identity details out of public
sources.

## Shell application configuration

| Setting | Source |
|---|---|
| Bash and Zsh interactive startup | [`bashrc`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bashrc), [`zshrc`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zshrc) |
| Bash and Zsh login profiles | [`bash_profile`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bash_profile), [`zprofile`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zprofile) |
| Shared shell environment and pager defaults | [`config/shell/profile.sh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/profile.sh) |
| Shared commands and aliases | [`config/shell/aliases.sh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/aliases.sh) |
| Zsh completion | [`config/zsh/completion.zsh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/completion.zsh) |
| Zsh command helpers | [`config/zsh/functions/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/zsh/functions/) |
| Git aliases | [`config/git/aliases`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases) |
| Download naming and timestamping | [`config/wgetrc`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/wgetrc) |

### Environment and paging

The login profiles source shared environment settings; interactive startup
files source shared aliases when present. Put
behavior common to Bash and Zsh in `config/shell/`; keep shell-specific
behavior in its own startup file. The [Windows guide](windows-and-wsl.md#powershell-configuration)
covers PowerShell's separate profile.

The shared profile supplies defaults for XDG directories, terminal information,
and the `less` pager. Customize these in `config/shell/profile.sh` or supply
environment overrides before startup. The source records each setting's defaults.

### Completion

Zsh uses normal completion on the first Tab. Repeat Tab to try approximate
matches.

Dotfiles command completion comes from the CLI's
[Shell completions task](task-reference.md#shell-completions), not a separate
hand-edited selector list.

### Git shortcuts

Common shortcuts include:

| Command | Purpose |
|---|---|
| `git st` | Show concise status and branch information |
| `git dis` | Review staged changes |
| `git lo` | Show a compact commit graph |
| `git gr <pattern>` | Search tracked content |

Run `git alias` to list all configured aliases.

Alias definitions belong in the linked Git files. Desired global key/value
overrides belong in [Git settings](configuration-reference.md#git-settings).

### Downloads and archives

Wget preserves URL filenames and uses timestamping to reuse unchanged
downloads. Change its shared defaults in `config/wgetrc`.

In Zsh, [`extract <file>`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/functions/extract)
chooses the extraction tool for the archive format. That tool must be installed;
RPM extraction uses `bsdtar`, supplied by `libarchive` on Arch.
Keep the extractor's normal path protections when customizing the helper.
Extract untrusted archives into a fresh directory and review the contents
before running anything; see [Security and trust](security-and-trust.md).
