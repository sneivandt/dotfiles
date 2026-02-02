# Neovim Plugin Management Migration

This directory contains configuration for Neovim with support for both traditional git submodules (vim pack) and modern lazy.nvim plugin management.

## Current Setup (Default)

By default, the configuration uses **git submodules** with Vim's native pack system (`~/.vim/pack/plugins/`). This is the traditional approach compatible with Vim 8+ and Neovim.

**Pros:**
- Works with both Vim and Neovim
- Plugins are committed as git submodules
- No external dependencies

**Cons:**
- Slower plugin loading
- Manual submodule management
- No lazy loading or lockfiles
- Harder to update individual plugins

## Modern Alternative: lazy.nvim (Optional)

For Neovim users, we provide optional support for **lazy.nvim**, a modern plugin manager with:
- Fast startup through lazy loading
- Lockfile support (`lazy-lock.json`)
- Automatic plugin installation
- Better dependency management
- Built-in plugin profiling

### How to Enable lazy.nvim

Set the environment variable before starting Neovim:

```bash
export NVIM_USE_LAZY=1
nvim
```

Or add to your shell profile (`~/.zshrc`, `~/.bashrc`):
```bash
# Use lazy.nvim for Neovim plugin management
export NVIM_USE_LAZY=1
```

### First-Time Setup

When you first launch Neovim with `NVIM_USE_LAZY=1`:

1. lazy.nvim will auto-install to `~/.local/share/nvim/lazy/lazy.nvim`
2. All plugins will be automatically downloaded
3. A lockfile will be created at `~/.config/nvim/lazy-lock.json`

### Plugin Management with lazy.nvim

Once enabled, you can use lazy.nvim commands:

- `:Lazy` - Open lazy.nvim UI
- `:Lazy update` - Update all plugins
- `:Lazy sync` - Install missing and update plugins
- `:Lazy clean` - Remove unused plugins
- `:Lazy profile` - Profile plugin loading times

### Configuration

Plugins are defined in `lua/lazy-bootstrap.lua`. The configuration mirrors the existing vim pack setup with all the same plugins.

## Migration Strategy

### Phase 1: Evaluation (Current)
- lazy.nvim support is **optional** via environment variable
- Git submodules remain the default
- Both approaches co-exist

### Phase 2: Transition (Future)
- Once lazy.nvim is validated, make it the default for Neovim
- Keep git submodules for Vim compatibility
- Update installation scripts to handle both

### Phase 3: Deprecation (Future)
- Fully migrate to lazy.nvim for Neovim
- Remove nvim-specific git submodules
- Maintain minimal vim support with essential plugins

## Files

- `init.vim` - Main entry point, sources `~/.vim/vimrc`
- `nvimrc` - Neovim-specific config, plugin loading logic
- `lua/lazy-bootstrap.lua` - lazy.nvim bootstrap and plugin definitions
- `pack/` - Git submodule plugins (default method)

## Troubleshooting

### Switching back to git submodules
Unset the environment variable and restart:
```bash
unset NVIM_USE_LAZY
nvim
```

### Plugin conflicts
If you see errors after switching modes:
1. Exit Neovim
2. Clean lazy.nvim cache: `rm -rf ~/.local/share/nvim/lazy`
3. Or reinit submodules: `cd ~/dotfiles && git submodule update --init --recursive`
