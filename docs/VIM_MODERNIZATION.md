# Vim Configuration Modernization

This document describes the modernization changes made to the vim/neovim configuration in this dotfiles repository.

## Summary of Changes

The vim configuration has been modernized to use current best practices and actively maintained plugins while maintaining compatibility with both Vim 8+ and Neovim.

## Plugin Changes

### Replaced Plugins

| Old Plugin | New Plugin | Reason |
|------------|-----------|---------|
| `ctrlp.vim` | `fzf.vim` | Much faster search (written in Go), better performance with large codebases, modern and actively maintained |
| `python-syntax` (hdima) | `vim-python-pep8-indent` | hdima's plugin is outdated; PEP8 indent is actively maintained. For Neovim, treesitter provides superior syntax highlighting |
| `vim-javascript-syntax` (jelera) | `vim-javascript` (pangloss) | Better maintained, more features, works well with modern JavaScript |

### New Plugins

| Plugin | Purpose | For |
|--------|---------|-----|
| `fzf` | Fast fuzzy finder core | Vim & Neovim |
| `fzf.vim` | Vim integration for fzf | Vim & Neovim |
| `vim-python-pep8-indent` | PEP8-compliant Python indentation | Vim & Neovim |
| `vim-javascript` | Modern JavaScript syntax | Vim & Neovim |
| `tokyonight.nvim` | Modern colorscheme alternative | Vim & Neovim |
| `gruvbox` | Warm, retro colorscheme | Vim & Neovim |
| `nvim-tree.lua` | Modern file explorer | Neovim only |
| `nvim-web-devicons` | File icons for nvim-tree | Neovim only |
| `lualine.nvim` | Fast statusline (3x faster than airline) | Neovim only |
| `nvim-treesitter` | Advanced syntax highlighting | Neovim only |
| `plenary.nvim` | Lua utilities (required by other plugins) | Neovim only |

### Unchanged Plugins (Still Good)

These plugins are still actively maintained and recommended:
- `vim-fugitive` - Git integration (tpope)
- `vim-commentary` - Commenting support (tpope)
- `vim-surround` - Surround text objects (tpope)
- `vim-dispatch` - Async build/test (tpope)
- `vim-easy-align` - Text alignment (junegunn)
- `vim-tmux-navigator` - Tmux/Vim navigation
- `vim-go` - Go development
- `vim-airline` - Statusline (for Vim; Neovim uses lualine)
- `nerdtree` - File explorer (for Vim; Neovim uses nvim-tree)
- `jellybeans.vim` - Colorscheme (classic, still good)
- `lessspace.vim` - Whitespace management
- `rust.vim` - Rust support
- `haskell-vim` - Haskell support
- `vim-cpp-enhanced-hilight` - C++ highlighting
- `vim-autotag` - Ctags automation

## Configuration Changes

### FZF Configuration

Replaced CtrlP with FZF for fuzzy finding:

```vim
" Key mappings
<C-p>      - FzfFiles (find files)
<leader>.  - FzfTags (find tags)
<leader>b  - FzfBuffers (find buffers)
<leader>/  - FzfRg (ripgrep search)
```

FZF automatically uses ripgrep if available for faster searching.

### Colorscheme

The colorscheme now has a fallback chain:
1. `jellybeans` (classic, preferred)
2. `tokyonight-night` (modern alternative)
3. `gruvbox` (warm, retro alternative)
4. `default` (final fallback)

### Neovim Enhancements

When running in Neovim, additional features are loaded:

#### nvim-tree (File Explorer)
- Modern, Lua-based file explorer
- Replaces NERDTree with better performance
- Shows Git status and file icons
- Toggle with `<C-e>`

#### lualine (Statusline)
- ~3x faster startup than vim-airline (~25ms vs ~80ms)
- Lua-native for better Neovim integration
- Automatic theme matching
- Includes tab line

#### Treesitter (Syntax Highlighting)
- AST-based syntax highlighting (more accurate than regex)
- Better performance
- Improved indentation
- Supports: Python, JavaScript, TypeScript, Lua, Vim, Rust, Go, C, C++, Java, Haskell, Bash

### Tmux Theme Update

The tmux theme has been refined to better align with the vim colorscheme:
- Cleaner status bar with better spacing
- Consistent color scheme (blue accent on colour04)
- Bold text for emphasis
- Message styling for better visibility

## Plugin Manager

The configuration continues to use Vim's native `pack/` plugin manager (Vim 8+):
- Plugins in `pack/plugins/start/` load automatically
- Neovim-specific plugins in `pack/plugins/opt/` load conditionally
- All plugins managed as git submodules

This approach is:
- Zero dependencies
- Stable and built-in
- Sufficient for this configuration's needs

Alternative plugin managers (vim-plug, lazy.nvim) were considered but the native approach provides good balance of simplicity and functionality.

## Compatibility

### Vim 8+
- All features work except Neovim-specific enhancements
- Uses NERDTree for file browsing
- Uses vim-airline for statusline
- Uses built-in syntax highlighting

### Neovim 0.5+
- Full feature set including Lua plugins
- Uses nvim-tree for file browsing
- Uses lualine for statusline
- Uses treesitter for syntax highlighting
- Automatic loading of enhanced features

## Performance Improvements

Based on benchmarks from the research:

| Component | Old | New | Improvement |
|-----------|-----|-----|-------------|
| Statusline startup | ~80ms (airline) | ~25ms (lualine) | 3.2x faster |
| Fuzzy finder | Slow (ctrlp) | Very fast (fzf) | Orders of magnitude faster on large repos |
| Syntax highlighting | Regex-based | AST-based (treesitter) | More accurate, better performance |

## Migration Notes

### For Existing Users

If you're upgrading from the old configuration:

1. **FZF Key Mappings**: `<C-p>` still works but now uses FZF instead of CtrlP. You can also use `:FzfFiles`, `:FzfBuffers`, etc.

2. **File Explorer**: 
   - Vim: Still uses NERDTree (`<C-e>` to toggle)
   - Neovim: Now uses nvim-tree (`<C-e>` to toggle)

3. **Colorscheme**: Still defaults to jellybeans, but modern alternatives available

4. **Python Syntax**: Treesitter provides better highlighting in Neovim; vim-python-pep8-indent handles indentation

### Dependencies

For the best experience, install these external tools:

- **fzf**: Fast fuzzy finder (core dependency)
  ```bash
  # On Arch Linux
  pacman -S fzf
  
  # On Ubuntu/Debian
  apt install fzf
  
  # On macOS
  brew install fzf
  ```

- **ripgrep**: Fast text search (optional but recommended)
  ```bash
  # On Arch Linux
  pacman -S ripgrep
  
  # On Ubuntu/Debian
  apt install ripgrep
  
  # On macOS
  brew install ripgrep
  ```

- **Neovim 0.5+**: For enhanced features (optional)

## Testing

The configuration has been tested with:
- Vim 9.1 (works correctly)
- Configuration loads without errors
- All legacy mappings preserved

To test the configuration:
```bash
# Test Vim
vim -c "source symlinks/vim/vimrc" -c "qa!"

# Test Neovim (if installed)
nvim -c "source ~/.vim/vimrc" -c "qa!"
```

## References

This modernization was based on research of current best practices in 2024-2025:
- [Slant: Best Vim Plugin Managers](https://www.slant.co/topics/1224/~best-plugin-managers-for-vim)
- [Slant: Best Vim Color Schemes](https://www.slant.co/topics/480/~best-vim-color-schemes)
- [nvim-lualine Performance Benchmarks](https://github.com/nvim-lualine/lualine.nvim)
- [FZF vs CtrlP Comparison](https://www.libhunt.com/compare-ctrlpvim--ctrlp.vim-vs-fzf.vim)
- [Treesitter vs Traditional Syntax](https://neovim.io/doc/user/treesitter.html)
