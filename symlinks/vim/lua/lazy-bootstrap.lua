-- Bootstrap lazy.nvim plugin manager
-- This file auto-installs lazy.nvim and loads plugin configurations

local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
if not vim.loop.fs_stat(lazypath) then
  -- Pin to a specific commit for security and reproducibility
  -- Update this commit hash periodically to get security fixes
  local lazy_commit = "077102c5bfc578693f12377846d427f49bc50076" -- v11.14.1 (2024-11-20)
  vim.fn.system({
    "git",
    "clone",
    "--filter=blob:none",
    "https://github.com/folke/lazy.nvim.git",
    lazypath,
  })
  vim.fn.system({
    "git",
    "-C",
    lazypath,
    "checkout",
    lazy_commit,
  })
end
vim.opt.rtp:prepend(lazypath)

-- Configure lazy.nvim with plugins
require("lazy").setup({
  -- Core plugins
  { "tpope/vim-commentary" },
  { "tpope/vim-surround" },
  { "tpope/vim-fugitive" },
  { "tpope/vim-dispatch" },

  -- File navigation and search
  { "junegunn/fzf", build = "./install --bin" },
  { "junegunn/fzf.vim" },
  { "christoomey/vim-tmux-navigator" },

  -- Editor enhancements
  { "junegunn/vim-easy-align" },
  { "thirtythreeforty/lessspace.vim" },

  -- Language support
  { "neovimhaskell/haskell-vim", ft = "haskell" },
  { "Vimjas/vim-python-pep8-indent", ft = "python" },
  { "pangloss/vim-javascript", ft = "javascript" },
  { "fatih/vim-go", ft = "go" },
  { "rust-lang/rust.vim", ft = "rust" },
  { "octol/vim-cpp-enhanced-highlight", ft = { "c", "cpp" } },

  -- Tags
  { "craigemery/vim-autotag" },

  -- Color schemes
  { "nanotech/jellybeans.vim" },
  { "morhetz/gruvbox" },
  { "folke/tokyonight.nvim", lazy = false, priority = 1000 },

  -- Neovim-specific plugins (only loaded in Neovim)
  {
    "nvim-tree/nvim-tree.lua",
    dependencies = { "nvim-tree/nvim-web-devicons" },
    cond = function() return vim.fn.has("nvim") == 1 end,
    config = function()
      require("nvim-tree").setup({
        disable_netrw = true,
        hijack_netrw = true,
        view = {
          width = 30,
          side = "right",
        },
        renderer = {
          icons = {
            show = {
              file = true,
              folder = true,
              folder_arrow = true,
              git = true,
            },
          },
        },
        filters = {
          dotfiles = false,
        },
      })
    end,
  },

  {
    "nvim-lualine/lualine.nvim",
    dependencies = { "nvim-tree/nvim-web-devicons" },
    cond = function() return vim.fn.has("nvim") == 1 end,
    config = function()
      require("lualine").setup({
        options = {
          theme = "auto",
          component_separators = "",
          section_separators = "",
        },
        sections = {
          lualine_a = {"mode"},
          lualine_b = {"branch", "diff"},
          lualine_c = {"filename"},
          lualine_x = {"encoding", "fileformat"},
          lualine_y = {"filetype"},
          lualine_z = {"location"}
        },
        tabline = {
          lualine_a = {},
          lualine_b = {},
          lualine_c = { { "tabs", mode = 1, max_length = vim.o.columns } },
          lualine_x = {},
          lualine_y = {},
          lualine_z = {}
        },
      })
    end,
  },

  {
    "nvim-treesitter/nvim-treesitter",
    cond = function() return vim.fn.has("nvim") == 1 end,
    build = ":TSUpdate",
    config = function()
      require("nvim-treesitter").setup({
        ensure_installed = {
          "python", "javascript", "typescript", "lua", "vim",
          "rust", "go", "c", "cpp", "java", "haskell", "bash"
        },
        highlight = {
          enable = true,
          additional_vim_regex_highlighting = false,
        },
        indent = {
          enable = true,
        },
      })
    end,
  },

  { "nvim-lua/plenary.nvim", cond = function() return vim.fn.has("nvim") == 1 end },
}, {
  -- Lazy.nvim configuration options
  defaults = {
    lazy = false, -- Load plugins on startup by default
  },
  install = {
    missing = true, -- Auto-install missing plugins
  },
  performance = {
    rtp = {
      disabled_plugins = {
        "gzip",
        "tarPlugin",
        "tohtml",
        "tutor",
        "zipPlugin",
      },
    },
  },
})
