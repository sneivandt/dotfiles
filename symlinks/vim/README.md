# Neovim Plugin Management

This directory contains configuration for Neovim and Vim with modern lazy.nvim plugin management for Neovim and traditional git submodules for Vim.

## Current Setup

The configuration uses **different approaches** based on the editor:

### Neovim (Default: lazy.nvim)

Neovim uses **lazy.nvim**, a modern plugin manager with:
- Fast startup through lazy loading
- Lockfile support (`lazy-lock.json`)
- Automatic plugin installation
- Better dependency management
- Built-in plugin profiling

**First-Time Setup:**

When you first launch Neovim:

1. lazy.nvim will auto-install to `~/.local/share/nvim/lazy/lazy.nvim`
2. All plugins will be automatically downloaded
3. A lockfile will be created at `~/.config/nvim/lazy-lock.json`

No configuration or environment variables needed - it works out of the box.

### Vim (Git Submodules)

Vim uses the traditional **git submodules** approach with Vim's native pack system (`~/.vim/pack/plugins/`):
- Compatible with Vim 8+
- Plugins are managed as git submodules
- No external dependencies required
- Manual submodule management via git commands

### Plugin Management with lazy.nvim (Neovim)

You can use lazy.nvim commands in Neovim:

- `:Lazy` - Open lazy.nvim UI
- `:Lazy update` - Update all plugins
- `:Lazy sync` - Install missing and update plugins
- `:Lazy clean` - Remove unused plugins
- `:Lazy profile` - Profile plugin loading times

### Configuration

Plugins are defined in `lua/lazy-bootstrap.lua`. The configuration includes all essential plugins for both Vim and Neovim, with Neovim-specific plugins conditionally loaded.

## Files

- `init.vim` - Main entry point, sources `~/.vim/vimrc`
- `nvimrc` - Plugin loading logic (lazy.nvim for Neovim, git submodules for Vim)
- `lua/lazy-bootstrap.lua` - lazy.nvim bootstrap and plugin definitions (Neovim only)
- `pack/` - Git submodule plugins (Vim only)

## Troubleshooting

### Plugin conflicts in Neovim
If you see errors with lazy.nvim:
1. Exit Neovim
2. Clean lazy.nvim cache: `rm -rf ~/.local/share/nvim/lazy`
3. Restart Neovim to re-download plugins

### Using git submodules for Vim
If you're using Vim (not Neovim), ensure submodules are initialized:
```bash
cd ~/dotfiles && git submodule update --init --recursive
```
