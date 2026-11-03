---
title: Vim and Neovim
description: Use the shared Vim configuration, understand Neovim plugins and parsers, and maintain the tracked plugin lockfile.
---

# Vim and Neovim

On Linux, `symlinks/vim/` is linked directly to both `~/.vim` and
`~/.config/nvim`. Vim uses the shared base configuration without third-party
plugins; Neovim adds plugins through lazy.nvim.

Because the installed paths point into the checkout, editing configuration or
updating the Neovim lockfile can modify tracked repository files.

On first use, starting Neovim downloads its plugin manager and missing plugins;
it is not a read-only configuration check. Git and network access are required.
Run `:TSInstallConfigured` separately for syntax parsers. See
[First launch](#first-launch) before opening the managed Neovim configuration.

## Everyday shortcuts

The leader key is comma: `,w` means press comma, then `w`. These shortcuts
use Normal mode unless another mode is named.

| Shortcut | Action | Editor |
|---|---|---|
| Ctrl-P | Find a file by name | Neovim |
| `,/` | Search file contents with ripgrep | Neovim |
| `,b` | Choose an open buffer | Neovim |
| Ctrl-E | Toggle the file tree | Neovim |
| `,t` | Open a terminal | Neovim |
| `,w` | Toggle line wrapping | Vim and Neovim |
| `,k` | Toggle English spell checking | Vim and Neovim |
| Space | Clear search highlighting | Vim and Neovim |
| `,v` | Open the editor's configuration entry point | Vim and Neovim |

File and content search use the Neovim plugins installed on
[first launch](#first-launch); content search also needs `rg` on PATH.

Several familiar Vim keys are remapped in both editors:

| Key | Behavior in this setup |
|---|---|
| `j` / `k` | Move by displayed lines, including wrapped lines |
| `H` / `L` | Move to the first / last nonblank character of the line |
| `J` / `K` | Move to the bottom / top line of the window, instead of joining lines / opening keyword help |
| `;` | Open the `:` command line |
| `/` / `?` | Search forward / backward with Vim's “very magic” regex syntax, where most punctuation has a special meaning |

See the shared mappings in
[vimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/vimrc)
and Neovim additions in
[nvimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/nvimrc)
for the full set.

## Configuration entry points

| File | Responsibility |
|---|---|
| [vimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/vimrc) | Shared editing behavior and mappings |
| [init.vim](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/init.vim) | Neovim entry point; sources `~/.vim/vimrc` and `~/.vim/nvimrc` |
| [nvimrc](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/nvimrc) | Neovim terminal behavior, plugin bootstrap, and additional mappings |
| [lua/lazy-bootstrap.lua](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/lua/lazy-bootstrap.lua) | lazy.nvim bootstrap, plugin declarations, and plugin configuration |
| [lazy-lock.json](https://github.com/sneivandt/dotfiles/blob/main/symlinks/vim/lazy-lock.json) | Tracked plugin revisions |

The links are declared in
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml).
They apply to Linux profiles, not only the desktop role. See
[Configuration reference](configuration-reference.md#symlinks) for their lifecycle
and [Package management](package-management.md) for installing the editors.

Neovim adds Tokyo Night, a file tree and status line, fuzzy file search,
Git integration, language support, and Tree-sitter highlighting. The plugin
declarations are the maintained inventory; plain Vim remains plugin-free.

## First launch

When lazy.nvim is absent, starting Neovim clones it into the editor's data
directory and checks out the commit specified in `lua/lazy-bootstrap.lua`.
The usual Linux location is `~/.local/share/nvim/lazy/lazy.nvim`.
Use `:echo stdpath('data')` in Neovim to find your data directory; lazy.nvim
lives under its `lazy/lazy.nvim` subdirectory.

Git and network access are required for bootstrap. Missing plugins are also
installed automatically. Launching Neovim is therefore not a read-only
configuration check.

The bootstrap commit matches lazy.nvim's revision in `lazy-lock.json`. It
selects the manager version for new installations; existing installations are
not reset on every launch. Review dependency changes and lockfile diffs as
code changes: a recorded revision does not establish that upstream code is safe.

### Tree-sitter parsers

Run `:TSInstallConfigured` once to install missing configured parsers. The
command starts installation asynchronously; wait for completion, then reopen
the file to enable highlighting and indentation.

Normal startup does not install parsers or wait for downloads. The plugin's
`:TSUpdate` build hook updates installed parsers during plugin updates. A
missing parser produces a warning directing you to `:TSInstallConfigured`;
it is not a reason to delete the plugin tree.

## Terminal navigation

In a Neovim terminal, Escape leaves terminal input mode and keeps the terminal
window focused so you can inspect output. Alt+Escape leaves terminal input and
returns to the previous window. Re-entering a terminal window restores input mode.

## Completion and undo history

Neovim 0.11+ uses fuzzy built-in insert completion; 0.12+ also ranks matches
by proximity to the cursor. On 0.12+, `,u` opens the built-in `:Undotree` to
navigate the persistent undo history. Plain Vim keeps its existing behavior.

## Plugin management

| Neovim command | Purpose |
|---|---|
| `:Lazy` | Inspect plugin state and errors |
| `:Lazy update` | Update plugins and their recorded revisions |
| `:Lazy sync` | Install missing plugins, clean unused ones, and update |
| `:Lazy clean` | Remove plugins no longer declared |
| `:Lazy profile` | Inspect plugin loading time |
| `:TSInstallConfigured` | Install missing parsers from the configured list |

Update commands change installed content and may change the tracked
`lazy-lock.json`. Review the repository diff before keeping those revisions;
do not run an update merely to inspect configuration.

To add or configure a plugin, edit its declaration in
`lua/lazy-bootstrap.lua`. Keep plugin-only behavior on the Neovim path so
plain Vim can still load without third-party plugins.

## Troubleshooting

Start with `:messages` and the error details in `:Lazy`. Bootstrap failures
include the failing Git command's output; check Git availability, connectivity,
and the reported revision before altering local state.

For a plugin-specific error, inspect that plugin's configuration and locked
revision first. Do not delete the entire lazy.nvim tree as a general repair:
that discards unrelated installed plugins and requires fresh downloads.
If a corrupt installation must be replaced, close Neovim, locate the exact
affected directory under the actual data path, and move it aside for recovery
before retrying.

## Maintenance

### Updating the bootstrap pin

Choose and review a lazy.nvim revision, then update the commit in
`lua/lazy-bootstrap.lua` and its entry in `lazy-lock.json` together, without
advancing unrelated plugin revisions. Verify first-launch behavior in an
isolated Neovim data directory: changing the source pin does not exercise
bootstrap when the manager is already installed. Check the resulting plugin
lockfile separately.

### Plugin-free regression checks

From the repository root, run:

```bash
python3 -B -m unittest discover -s symlinks/vim/tests -p 'test_*.py'
```

These checks use isolated homes and stub plugin loading to cover shared
configuration, startup, and mappings without downloading plugins. They also
check that the bootstrap pin matches the tracked lockfile.

CI restores plugins with `:Lazy! restore`, verifies installed commits against
the tracked lockfile, and rejects lockfile changes during application tests.
Plugin updates remain an explicit maintenance action.

Use [Testing](testing.md) for repository validation and application
coverage. Avoid using the installed editor profile for checks that are meant
to leave the live configuration unchanged.
