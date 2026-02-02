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
              file = false,
              folder = false,
              folder_arrow = true,
              git = false,
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
    cond = function() return vim.fn.has("nvim") == 1 end,
    config = function()
      require("lualine").setup({
        options = {
          theme = "tokyonight",
          component_separators = { left = "|", right = "|" },
          section_separators = { left = "", right = "" },
          globalstatus = true,
          icons_enabled = false,
        },
        sections = {
          lualine_a = { "mode" },
          lualine_b = { "branch", "diff", "diagnostics" },
          lualine_c = { { "filename", path = 1 } },
          lualine_x = { "encoding", "fileformat" },
          lualine_y = { "filetype" },
          lualine_z = { "location" }
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
      local ok, treesitter = pcall(require, "nvim-treesitter.configs")
      if ok then
        treesitter.setup({
          ensure_installed = {
            "python", "javascript", "typescript", "lua", "vim", "vimdoc",
            "rust", "go", "c", "cpp", "java", "haskell", "bash",
            "json", "yaml", "toml", "markdown", "markdown_inline"
          },
          highlight = {
            enable = true,
            additional_vim_regex_highlighting = false,
          },
          indent = {
            enable = true,
          },
          incremental_selection = {
            enable = true,
            keymaps = {
              init_selection = "<CR>",
              scope_incremental = "<CR>",
              node_incremental = "<TAB>",
              node_decremental = "<S-TAB>",
            },
          },
        })
      end
    end,
  },

  { "nvim-lua/plenary.nvim", cond = function() return vim.fn.has("nvim") == 1 end },

  -- Modern indent guides
  {
    "lukas-reineke/indent-blankline.nvim",
    main = "ibl",
    cond = function() return vim.fn.has("nvim") == 1 end,
    config = function()
      require("ibl").setup({
        indent = {
          char = "|",
          tab_char = "|",
        },
        scope = {
          enabled = true,
          show_start = false,
          show_end = false,
        },
        exclude = {
          filetypes = {
            "help",
            "alpha",
            "dashboard",
            "neo-tree",
            "Trouble",
            "lazy",
            "mason",
            "notify",
            "toggleterm",
            "lazyterm",
          },
        },
      })
    end,
  },

  -- Git integration with inline blame and signs
  {
    "lewis6991/gitsigns.nvim",
    cond = function() return vim.fn.has("nvim") == 1 end,
    config = function()
      require("gitsigns").setup({
        signs = {
          add          = { text = "+" },
          change       = { text = "~" },
          delete       = { text = "_" },
          topdelete    = { text = "-" },
          changedelete = { text = "~" },
          untracked    = { text = "|" },
        },
        signcolumn = true,
        numhl = false,
        linehl = false,
        word_diff = false,
        watch_gitdir = {
          interval = 1000,
          follow_files = true
        },
        current_line_blame = false,
        current_line_blame_opts = {
          virt_text = true,
          virt_text_pos = "eol",
          delay = 1000,
        },
        on_attach = function(bufnr)
          local gs = package.loaded.gitsigns

          local function map(mode, l, r, opts)
            opts = opts or {}
            opts.buffer = bufnr
            vim.keymap.set(mode, l, r, opts)
          end

          -- Navigation
          map("n", "]c", function()
            if vim.wo.diff then return "]c" end
            vim.schedule(function() gs.next_hunk() end)
            return "<Ignore>"
          end, {expr = true})

          map("n", "[c", function()
            if vim.wo.diff then return "[c" end
            vim.schedule(function() gs.prev_hunk() end)
            return "<Ignore>"
          end, {expr = true})

          -- Actions
          map("n", "<leader>hs", gs.stage_hunk)
          map("n", "<leader>hr", gs.reset_hunk)
          map("v", "<leader>hs", function() gs.stage_hunk {vim.fn.line("."), vim.fn.line("v")} end)
          map("v", "<leader>hr", function() gs.reset_hunk {vim.fn.line("."), vim.fn.line("v")} end)
          map("n", "<leader>hS", gs.stage_buffer)
          map("n", "<leader>hu", gs.undo_stage_hunk)
          map("n", "<leader>hR", gs.reset_buffer)
          map("n", "<leader>hp", gs.preview_hunk)
          map("n", "<leader>hb", function() gs.blame_line{full=true} end)
          map("n", "<leader>tb", gs.toggle_current_line_blame)
          map("n", "<leader>hd", gs.diffthis)
          map("n", "<leader>hD", function() gs.diffthis("~") end)
          map("n", "<leader>td", gs.toggle_deleted)
        end
      })
    end,
  },
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
