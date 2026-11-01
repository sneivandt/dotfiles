---
title: Application settings
description: Find the right place for application preferences and customize shared shell behavior, completion, Git shortcuts, and downloads.
---

# Application settings

Application preferences live in tracked files under `symlinks/`. Edit those
sources, not installed copies or generated files: a home-directory link may
make this checkout live configuration, so some changes take effect immediately.

Use [Configuration reference](configuration-reference.md#symlinks) to change
**which files are installed**, and [Private overlays](private-overlays.md) for
machine-specific alternatives. This page covers shared shell preferences;
desktop and editor workflows have their own [subsystem guides](#desktop-and-editors).

## Shell application configuration

| Setting | Source |
|---|---|
| Shared shell environment and pager defaults | [`config/shell/profile.sh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/profile.sh) |
| Zsh completion | [`config/zsh/completion.zsh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/completion.zsh) |
| Git aliases | [`config/git/aliases`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases) |
| Download naming and timestamping | [`config/wgetrc`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/wgetrc) |

### Environment and paging

The shared profile keeps nonempty supplied `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `TERMINFO` and `TERMINFO_DIRS` values. Unset **or empty** values
use home-directory defaults. The default terminal search path also retains
the system terminfo database.

`LESS` is different: a supplied value, including an empty one, is preserved.
The default is `-imRj8`; add `-X` if you want pager output left in terminal
scrollback.

### Completion

Zsh attempts approximate completion only after a repeated Tab with unchanged
input, cursor position and history entry. A first Tab does not immediately
fall back to approximate matches.

Dotfiles command completion comes from the CLI's
[Shell completions task](task-reference.md#shell-completions), not a separate
hand-edited selector list.

### Git shortcuts

- `git gr <pattern>` searches case-insensitively with line numbers. Color is
  automatic for terminal output, not forced into redirected output.
- `git contributors` summarizes `HEAD`. Pass a revision range such as
  `main..HEAD` to select different commits.

Alias definitions belong in the linked Git files. Desired global key/value
overrides belong in [Git settings](configuration-reference.md#git-settings).

### Downloads

Wget preserves URL filenames and uses timestamping to reuse unchanged
downloads. Request these behaviors explicitly when needed:

| Flag | Use it for |
|---|---|
| `--adjust-extension` | Web-page filename extensions |
| `--trust-server-names` | Redirected filenames |
| `--content-disposition` | Names supplied by response headers |

Extension adjustment can prevent timestamping from recognizing a file
previously downloaded from the same URL.

## Desktop and editors

- [Arch desktop](arch-desktop.md): session services, appearance, the bar and
  menus, shortcuts, locking, and reloads.
- [Windows and WSL](windows-and-wsl.md): Windows integration, PowerShell,
  permissions, and the boundary between Windows and Linux homes.
- [Visual Studio Code](visual-studio-code.md): shared settings, extension
  installation, Insiders selection, and remote environments.
- [Vim and Neovim](vim-and-neovim.md): shared editing behavior, plugin
  bootstrap, parsers, and deliberate dependency updates.
- [AI agent configuration](ai-agent-configuration.md): harness preferences,
  reusable plugins, configuration fragments, and deployments.
