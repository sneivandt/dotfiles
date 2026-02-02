# Neovim Plugin Management

This directory contains configuration for Neovim with modern lazy.nvim plugin management.

## Current Setup

### Neovim (lazy.nvim)

Neovim uses **lazy.nvim**, a modern plugin manager with:
- Fast startup through lazy loading
- Lockfile support (`lazy-lock.json`)
- Automatic plugin installation
- Better dependency management
- Built-in plugin profiling
- Pinned to a specific commit for security and reproducibility

**First-Time Setup:**

When you first launch Neovim:

1. lazy.nvim will auto-install to `~/.local/share/nvim/lazy/lazy.nvim`
2. All plugins will be automatically downloaded
3. A lockfile will be created at `~/.config/nvim/lazy-lock.json`

No configuration or environment variables needed - it works out of the box.

**Security Note:** The bootstrap process pins lazy.nvim to a specific commit hash to prevent supply-chain attacks. The commit is periodically updated to get security fixes.

### Vim (No Plugins)

Regular Vim (non-Neovim) runs without plugins for simplicity. All plugin functionality is provided by Neovim through lazy.nvim.

### Plugin Management with lazy.nvim

You can use lazy.nvim commands in Neovim:

- `:Lazy` - Open lazy.nvim UI
- `:Lazy update` - Update all plugins
- `:Lazy sync` - Install missing and update plugins
- `:Lazy clean` - Remove unused plugins
- `:Lazy profile` - Profile plugin loading times

### Configuration

Plugins are defined in `lua/lazy-bootstrap.lua`. The configuration includes all essential plugins for Neovim development.

## Files

- `init.vim` - Main entry point, sources `~/.vim/vimrc`
- `nvimrc` - Plugin loading logic for Neovim
- `lua/lazy-bootstrap.lua` - lazy.nvim bootstrap and plugin definitions (Neovim only)

## Troubleshooting

### Plugin conflicts in Neovim
If you see errors with lazy.nvim:
1. Exit Neovim
2. Clean lazy.nvim cache: `rm -rf ~/.local/share/nvim/lazy`
3. Restart Neovim to re-download plugins

### Updating lazy.nvim
To update to a newer version of lazy.nvim:
1. Check the latest stable release at https://github.com/folke/lazy.nvim/releases
2. Update the commit hash in `lua/lazy-bootstrap.lua`
3. Remove the lazy.nvim directory: `rm -rf ~/.local/share/nvim/lazy/lazy.nvim`
4. Restart Neovim to re-download
