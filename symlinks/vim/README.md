# Vim and Neovim configuration

On Linux, this directory is linked directly to both `~/.vim` and
`~/.config/nvim`. Vim uses the shared base configuration without third-party
plugins; Neovim adds plugins through lazy.nvim.

Because the installed paths point into the checkout, editing configuration or
updating the Neovim lockfile can modify tracked repository files.

## Configuration entry points

| File | Responsibility |
|---|---|
| [`vimrc`](vimrc) | Shared editing behavior and mappings |
| [`init.vim`](init.vim) | Neovim entry point; sources `~/.vim/vimrc` and `~/.vim/nvimrc` |
| [`nvimrc`](nvimrc) | Neovim terminal behavior, plugin bootstrap, and additional mappings |
| [`lua/lazy-bootstrap.lua`](lua/lazy-bootstrap.lua) | lazy.nvim bootstrap, plugin declarations, and plugin configuration |
| [`lazy-lock.json`](lazy-lock.json) | Tracked plugin revisions |

The links are declared in
[`conf/symlinks.toml`](../../conf/symlinks.toml). See
[Configuration](../../docs/CONFIGURATION.md#symlinks) for their lifecycle.

## First launch

When lazy.nvim is absent, starting Neovim clones it into
`stdpath("data") .. "/lazy/lazy.nvim"` and checks out the commit specified in
`lua/lazy-bootstrap.lua`. The usual Linux location is
`~/.local/share/nvim/lazy/lazy.nvim`; use `:echo stdpath('data')` to find the
actual data directory rather than assuming the default.

Git and network access are required for bootstrap. Missing plugins are also
installed automatically. Launching Neovim is therefore not a read-only
configuration check.

The bootstrap commit fixes which lazy.nvim revision a new installation uses;
it is not a guarantee that upstream code is safe, nor is an existing
installation reset to that revision on every launch. Review dependency changes
and lockfile diffs as code changes.

### Tree-sitter parsers

Run `:TSInstallConfigured` once to install missing configured parsers. The
command starts installation asynchronously; wait for completion, then reopen
the file to enable highlighting and indentation.

Normal startup does not install parsers or wait for downloads. The plugin's
`:TSUpdate` build hook updates installed parsers during plugin updates. A
missing parser produces a warning directing you to `:TSInstallConfigured`;
it is not a reason to delete the plugin tree.

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

### Updating the bootstrap pin

Choose and review a lazy.nvim revision, then update the commit in
`lua/lazy-bootstrap.lua`. Verify first-launch behavior in an isolated Neovim
data directory: changing the source pin does not exercise bootstrap when the
manager is already installed. Check the resulting plugin lockfile separately.

Use [Testing](../../docs/TESTING.md) for repository validation and application
coverage. Avoid using the installed editor profile for checks that are meant
to leave the live configuration unchanged.
