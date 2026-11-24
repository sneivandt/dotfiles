---
title: Application settings
description: Customize Git, shell prompts and completion, Vim, Neovim, and VS Code through tracked sources, and back up or restore existing application settings.
---

# Application settings

Application preferences live in tracked files under
[symlinks/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/). Always
edit those tracked sources rather than installed copies or generated files.
Because home symlinks point straight back into the checkout, many edits take
effect as soon as you save.

Installing software and configuring it are separate concerns:
[Package management](package-management.md) chooses which applications are
installed, [symlink declarations](../reference/home-symlinks.md#configuration)
map tracked files into your home directory, and the files themselves govern how
each tool behaves. Use the quick table below to jump to the right source, then
read the sections on Git, shells, VS Code, and Vim/Neovim.

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

Every source path in this table lives beneath `symlinks/`. Clicking a link
opens the public file on GitHub, so make your actual edits in your local
checkout. Home paths like `~/.config/git/config` are the installed links rather
than separate copies to edit.

## Make the change

When changing a preference in a file that is already linked, edit the tracked
source and let the application reload or restart normally; you do not need to
re-run `dotfiles update`. Keep in mind that interactive settings UIs (including
VS Code's) can write directly back through a symlink into your checkout, so
check `git diff` afterward.

To link a new file or add a destination, declare it in
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml)
and preview the link task:

```sh
dotfiles update --only symlinks --dry-run
```

Existing files, directories, and unrelated links are reported as conflicts so
nothing is silently overwritten; back them up and move them aside before
applying. For machine-specific or private alternatives, use a
[private overlay](private-overlays.md) (which replaces the linked source whole
rather than merging file contents), and keep credentials and private identity
details out of the public checkout.

### Back up and restore a file

Before applying dotfiles for the first time, back up any existing file at a
path you plan to manage. On Linux, for example, back up `~/.config/git/config`
if it exists and is readable:

```sh
backup_dir=$(mktemp -d "$HOME/dotfiles-backup.XXXXXX")
cp -pL "$HOME/.config/git/config" "$backup_dir/config"
printf 'Backup: %s\n' "$backup_dir/config"
```

Check the backup before applying. Passing `-L` copies the actual file contents
even if the existing path is a symlink; if it is a symlink and you might want
that exact link back later, record its target with `readlink` as well. Back up
any other targeted files separately, including files included by your Git
config. On Windows, copy the resolved file contents to a separate backup folder
and record any original symlink target.

To restore a backup later, first stop selecting that link in
`conf/symlinks.toml`. Move the current symlink itself aside, then copy your
backup into place as an ordinary file (copying straight onto an active symlink
would overwrite the tracked file in the checkout). If you want the original
symlink back instead, recreate it only after confirming its recorded target
still exists, then verify what Git reads with
`git config --show-origin --get user.name`.

## Git

Shared Git identity and defaults live in
[config/git/config](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/config),
and aliases live in
[config/git/aliases](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases).
Review the configured identity before applying. Note that the shared config
rewrites GitHub HTTPS push URLs to SSH, so pushing requires your own SSH
authentication. Because these settings apply across all repositories on the
machine, follow the
[overlay file example](private-overlays.md#replace-an-application-file) if you
need a separate work identity or custom defaults.

If you want dotfiles to set individual global Git keys directly, declare them
in [conf/git-config.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/git-config.toml)
(see [Global Git settings](../reference/global-git-settings.md#configuration)
for platform rules and Windows `core.autocrlf` cleanup). Avoid setting
conflicting values in both the linked config and `git-config.toml`.

| Shortcut | Purpose |
|---|---|
| `git st` | Show concise status and branch information |
| `git dis` | Review staged changes |
| `git lo` | Show a compact commit graph |
| `git gr <pattern>` | Search tracked content |

Run `git alias` to see the complete list. If a setting behaves unexpectedly,
run `git config --show-origin --get <key>` inside the affected repository to
see whether an included file or repository-local value is overriding the shared
default.

`git delete-merged` removes local branches already merged into `HEAD`, leaving
tags, the current branch, `main`, `master`, `dev`, and `develop` intact while
enforcing Git's standard `branch -d` safety check. Unlike the template hook
below, it compares against `HEAD` rather than `origin/HEAD`.

On Windows, the `base` profile leaves Git's editor choice to your environment,
while `desktop` adds `config/git/windows-desktop` to select VS Code Insiders.
Switching back to `base` leaves an already-installed desktop link in place, so
remove that link deliberately if you no longer want it.

### Git templates

The shared Git config points `init.templateDir` at
[symlinks/config/git/templates/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git/templates/).
Whenever you initialize or clone a repository, Git copies those hook
entrypoints while their helper scripts resolve from the managed directory at
runtime. These personal templates are separate from this repository's
[contributor hooks](../development/contributing.md#git-hooks). Existing clones
only pick up new template entrypoints if you install them explicitly without
overwriting repository-specific hooks.

**The templates can automatically delete local branches.** After a merge or
rebase, [branch cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/templates/hooks.local/clean-merged-branches)
prunes local branches merged into `origin/HEAD` using `git branch -d`. It
preserves the current branch, the default branch, `main`, `master`, `dev`,
`develop`, and any branch checked out in a worktree. It never touches remote
branches, skips silently if `origin/HEAD` is absent or after an amend, and
leaves every branch alone if Git cannot finish listing branches or worktrees.

The [push-protection hook](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/templates/hooks.local/pre-push)
guards `main`, `master`, `develop`, and the push remote's default branch. You
can protect extra branch names with repository-local `dotfiles.protectedBranch`
entries. GitHub remotes whose URL owner matches `github.user` (or fallback
`github.username`) are exempt based on that local name check rather than a live
GitHub permission lookup. When fetch and push URLs differ, the hook inspects
the push remote directly instead of trusting the fetch remote's cached default
branch, and it blocks the push if Git cannot read the destination or protection
settings.

## Shell configuration

| Setting | Source |
|---|---|
| Bash and Zsh interactive startup | [bashrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bashrc), [zshrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zshrc) |
| Bash and Zsh login profiles | [bash_profile](https://github.com/sneivandt/dotfiles/blob/main/symlinks/bash_profile), [zprofile](https://github.com/sneivandt/dotfiles/blob/main/symlinks/zprofile) |
| Shared shell environment and pager defaults | [config/shell/profile.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/profile.sh) |
| Shared commands and aliases | [config/shell/aliases.sh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/aliases.sh) |
| Zsh completion | [config/zsh/completion.zsh](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/completion.zsh) |
| Zsh command helpers | [config/zsh/functions/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/zsh/functions/) |

### Environment and paging

Login shells load the shared environment setup, and interactive shells load the
shared aliases. Keep cross-shell behavior in `config/shell/` and shell-specific
tweaks in the corresponding Bash or Zsh startup file. PowerShell has its own
profile under [PowerShell configuration](windows-and-wsl.md#powershell-configuration).

To preserve any login setup you had before installing dotfiles, the managed
`~/.bash_profile` sources `~/.bash_login` first (falling back to `~/.profile`
if `~/.bash_login` is absent or unreadable).

`config/shell/profile.sh` provides defaults for XDG directories, editor,
locale, terminal settings, and `less`, while keeping any existing `EDITOR`,
`VISUAL`, `LANG`, and XDG values intact. Managed `.config` symlinks always
target `~/.config`, so setting `XDG_CONFIG_HOME` elsewhere triggers a shell
warning and fails `dotfiles check` and symlink installation; unset it or point
it at `~/.config` before applying.

Explicit `WGETRC` and `INPUTRC` values are kept as-is. When unset, the profile
checks the XDG directory first and then the managed `~/.config` path, leaving
the variable unset if neither file exists so tools don't break on missing
paths. Other defaults, including pager colors, are applied on startup.

`config/shell/path.sh` prepends user command directories to `PATH` without
duplicating entries, honoring `CARGO_HOME`, `GOBIN`, and `GOPATH`. This shell
startup ordering is separate from the CLI's
[Shell PATH task](../reference/shell-path.md), which persists `~/.local/bin`
for the dotfiles launcher.

### Completion

In Zsh, pressing Tab once uses normal completion; pressing Tab again tries
approximate matching.

Tab completion for `dotfiles` itself is generated by the
[Shell completions task](../reference/shell-completions.md) directly from the
running CLI rather than maintained by hand.

## Visual Studio Code

The `desktop` profile installs VS Code Insiders on Arch and Windows and links
shared settings into both stable and Insiders user directories (even if only
one edition is installed). Edit
[settings.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/Code/User/settings.json)
and
[keybindings.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/Code/User/keybindings.json)
to adjust the Tokyo Night theme, Source Code Pro font, word wrapping, save on
focus change, and default terminal (Zsh on Linux, PowerShell on Windows).
Editor and extension updates stay manual, and changing settings inside the VS
Code UI writes back through the symlink, so review `git diff` afterward.

Under the `desktop` profile, dotfiles links these paths beneath your home
directory (`base` omits them):

- Linux: `.config/Code/User/` and `.config/Code - Insiders/User/`
- Windows: `AppData/Roaming/Code/User/` and `AppData/Roaming/Code - Insiders/User/`
- Remote servers: `.vscode-server/data/Machine/settings.json` and
  `.vscode-server-insiders/data/Machine/settings.json`

Remote server paths receive `settings.json` only. Run dotfiles inside each host
home that owns those directories: configuring Windows does not configure WSL or
a remote Linux box, and workspace settings can still override user settings.

### Extensions

Declare Marketplace extension IDs in `conf/vscode-extensions.toml` using the
[VS Code extensions task](../reference/vs-code-extensions.md), which covers the
declaration format, editor CLI preference, previews, and first-login behavior.
For a fresh setup, run a normal update so package prerequisites are already in
place.

The list installs missing extensions without pinning versions or uninstalling
removed IDs; use VS Code's Extensions view for upgrades and removals, or add
personal IDs in a private overlay.

On Arch, the managed Insiders launcher and URL handler store credentials in
GNOME libsecret (see
[GNOME Keyring stays locked](troubleshooting.md#gnome-keyring-stays-locked) if
sign-in fails). During chroot provisioning, extension installation is deferred
to the first graphical login when the first-login user service is enabled.

If extension installation fails, inspect
`dotfiles log --id <run-id> --task vscode-extensions --verbose` and check the
selected CLI, profile, home path, symlink targets, and workspace overrides
before reinstalling anything. Coding-agent preferences and skills are managed
separately in [Agent configuration](agent-configuration.md).

## Vim and Neovim

On Linux, `symlinks/vim/` is linked to both `~/.vim` and `~/.config/nvim`
across both `base` and `desktop` profiles. Plain Vim uses the shared settings
with no third-party plugins, while Neovim loads lazy.nvim plugins. Because
those directories link directly into the checkout, edits and plugin lockfile
updates appear in `git diff`.

| Source | Change here |
|---|---|
| [vimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/vimrc) | Shared behavior and mappings |
| [init.vim](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/init.vim) | Neovim entry point |
| [nvimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/nvimrc) | Neovim mappings and terminal behavior |
| [lazy-bootstrap.lua](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/lua/lazy-bootstrap.lua) | Plugin declarations, configuration, and bootstrap pin |
| [lazy-lock.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/lazy-lock.json) | Recorded plugin revisions |

The leader key is comma. Shared mappings include `,w` (toggle wrapping), `,k`
(spell checking), and `,v` (open configuration). Neovim adds Ctrl-P (file
search), `,/` (ripgrep search), Ctrl-E (file tree), `,b` (buffers), and `,t`
(terminal). Inside a Neovim terminal, Escape leaves input mode and Alt+Escape
jumps back to the previous window. Check the source files for the complete list
of remapped navigation and search keys.

### First launch and plugins

Launching Neovim for the first time clones the pinned lazy.nvim plugin manager
and installs missing plugins. That requires Git and network access, so starting
Neovim is **not a read-only check**. Existing installs are never reset on
startup; run `:echo stdpath('data')` to see where `lazy/lazy.nvim` lives.

<details>
<summary>Bootstrap staging and lock recovery</summary>

Fresh installs clone and check out into a private sibling directory before
moving into place. If an earlier bootstrap left a manager missing its runtime
files, startup moves it aside as `lazy.nvim.bootstrap-*.interrupted` and
installs cleanly; inspect any backup before deleting it.

Concurrent startups may clone in parallel, with only one publishing the final
directory. An interrupted clone can leave an unused `lazy.nvim.bootstrap-*`
staging directory. Publication locks record their owning PID atomically and
recover once that process exits; if startup cannot acquire the lock, wait for
the other Neovim process to exit and retry. Abandoned staging directories can
be removed once no Neovim process is using that data path.

</details>

Run `:TSInstallConfigured` to install missing Tree-sitter parsers, wait for the
background install to finish, and reopen the file. Regular startup does not
download parsers, though `:TSUpdate` refreshes installed parsers during plugin
updates.

Use `:Lazy` to inspect plugin status and errors, `:Lazy update` to bump
revisions, and `:Lazy sync` to install missing plugins and prune unused ones.
Review `git diff` afterward since updates can modify `lazy-lock.json`. Keep
plugin-only configuration on the Neovim path so plain Vim continues to start
cleanly on its own. Neovim 0.11+ enables fuzzy insert completion, and 0.12+
adds proximity ranking plus `,u` for the built-in undo tree.

When a plugin errors out, check `:messages`, `:Lazy`, the failing Git command,
and the plugin's locked revision rather than wiping the entire plugin tree. If
a single plugin directory is corrupted, close Neovim and move only that
directory aside under your data path before retrying.

If you change bootstrap or plugin-loading behavior, keep the separate Vim and
Neovim entrypoints intact, touch the bootstrap pin or lockfile only when
intended, and verify startup with the isolated
[application configuration tests](../development/testing.md#application-configuration)
instead of your live home directory.

To configure AI coding agents next, continue to
[Agent configuration](agent-configuration.md), or jump to
[Private overlays](private-overlays.md) to keep personal overrides outside the
public repository.
