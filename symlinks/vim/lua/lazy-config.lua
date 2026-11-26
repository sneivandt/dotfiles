local plugins = {
  -- Core editing and Git commands
  { "tpope/vim-surround", event = "VeryLazy" },
  {
    "tpope/vim-fugitive",
    cmd = {
      "G", "Git", "Gdiffsplit", "Gread", "Gwrite", "Ggrep",
      "GMove", "GDelete", "GBrowse", "GRemove", "GRename", "Glgrep", "Gedit",
    },
  },
  {
    "tpope/vim-dispatch",
    cmd = { "Dispatch", "Make", "Focus", "Start", "Spawn" },
  },

  -- File navigation and search
  {
    "junegunn/fzf.vim",
    dependencies = { "junegunn/fzf" },
    cmd = {
      "FzfFiles", "FzfGFiles", "FzfBuffers", "FzfRg", "FzfLines", "FzfBLines",
      "FzfTags", "FzfBTags", "FzfMarks", "FzfWindows", "FzfLocate", "FzfHistory",
      "FzfCommits", "FzfBCommits", "FzfCommands", "FzfMaps", "FzfHelptags", "FzfFiletypes",
    },
  },
  {
    "christoomey/vim-tmux-navigator",
    cmd = {
      "TmuxNavigateLeft", "TmuxNavigateDown", "TmuxNavigateUp",
      "TmuxNavigateRight", "TmuxNavigatePrevious", "TmuxNavigatorProcessList",
    },
    keys = {
      { "<c-h>", "<cmd><C-U>TmuxNavigateLeft<cr>" },
      { "<c-j>", "<cmd><C-U>TmuxNavigateDown<cr>" },
      { "<c-k>", "<cmd><C-U>TmuxNavigateUp<cr>" },
      { "<c-l>", "<cmd><C-U>TmuxNavigateRight<cr>" },
      { "<c-\\>", "<cmd><C-U>TmuxNavigatePrevious<cr>" },
    },
  },

  -- Editor enhancements
  {
    "junegunn/vim-easy-align",
    cmd = "EasyAlign",
    keys = {
      { "<Enter>", "<Plug>(EasyAlign)", mode = "v" },
      { "ga", "<Plug>(EasyAlign)", mode = "n" },
    },
  },
  { "thirtythreeforty/lessspace.vim", event = { "BufReadPre", "BufNewFile" } },

  -- Language support (Go formatting/tooling; syntax & indent use Tree-sitter)
  { "fatih/vim-go", ft = "go" },

  -- Color scheme
  {
    "folke/tokyonight.nvim",
    lazy = false,
    priority = 1000,
    config = function()
      vim.cmd("colorscheme tokyonight-night")
      vim.cmd("highlight CursorLine guibg=#24283b ctermbg=235")
      vim.cmd("highlight CursorLineNr guifg=#7aa2f7 gui=bold ctermfg=4")
      vim.cmd("highlight LineNr guifg=#565f89 ctermfg=239")
      vim.cmd("highlight SignColumn guibg=#1a1b26 ctermbg=234")
      vim.cmd("highlight VertSplit guifg=#414868 guibg=#1a1b26 ctermfg=239 ctermbg=234")
      vim.cmd("highlight WinSeparator guifg=#414868 guibg=#1a1b26 ctermfg=239 ctermbg=234")
    end,
  },

  -- Neovim-specific plugins
  {
    "nvim-tree/nvim-tree.lua",
    cmd = { "NvimTreeToggle", "NvimTreeOpen", "NvimTreeFocus", "NvimTreeFindFile", "NvimTreeClose" },
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
            glyphs = {
              folder = {
                arrow_closed = "+",
                arrow_open = "-",
              },
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
    lazy = false,
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
          lualine_c = { { "tabs", mode = 1, path = 3, max_length = function() return vim.o.columns end } },
          lualine_x = {},
          lualine_y = {},
          lualine_z = {}
        },
      })
    end,
  },

  {
    "nvim-treesitter/nvim-treesitter",
    lazy = false,
    build = ":TSUpdate",
    config = function()
      local treesitter = require("nvim-treesitter")
      local parsers = {
        "python", "javascript", "typescript", "lua", "vim", "vimdoc",
        "rust", "go", "c", "cpp", "java", "haskell", "bash",
        "json", "yaml", "toml", "markdown", "markdown_inline",
      }
      vim.api.nvim_create_user_command("TSInstallConfigured", function()
        treesitter.install(parsers)
      end, { desc = "Install configured Tree-sitter parsers" })

      local group = vim.api.nvim_create_augroup("TreesitterConfig", { clear = true })
      vim.api.nvim_create_autocmd("FileType", {
        group = group,
        pattern = {
          "python", "javascript", "typescript", "lua", "vim", "vimdoc", "help",
          "rust", "go", "c", "cpp", "java", "haskell", "sh",
          "json", "yaml", "toml", "markdown",
        },
        callback = function(args)
          local started, message = pcall(vim.treesitter.start, args.buf)
          if not started then
            vim.notify(
              "Tree-sitter failed for " .. vim.bo[args.buf].filetype .. ": " .. message
                .. ". Run :TSInstallConfigured if parsers are missing.",
              vim.log.levels.WARN
            )
            return
          end
          vim.bo[args.buf].indentexpr = "v:lua.require'nvim-treesitter'.indentexpr()"
        end,
      })
    end,
  },

  -- Modern indent guides
  {
    "lukas-reineke/indent-blankline.nvim",
    event = { "BufReadPost", "BufNewFile" },
    main = "ibl",
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
            "lazy",
            "NvimTree",
          },
        },
      })
    end,
  },

  -- Git integration with inline blame and signs
  {
    "lewis6991/gitsigns.nvim",
    event = { "BufReadPre", "BufNewFile" },
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
            vim.schedule(function() gs.nav_hunk("next") end)
            return "<Ignore>"
          end, {expr = true})

          map("n", "[c", function()
            if vim.wo.diff then return "[c" end
            vim.schedule(function() gs.nav_hunk("prev") end)
            return "<Ignore>"
          end, {expr = true})

          -- Actions
          map("n", "<leader>hs", gs.stage_hunk)
          map("n", "<leader>hr", gs.reset_hunk)
          map("v", "<leader>hs", function() gs.stage_hunk {vim.fn.line("."), vim.fn.line("v")} end)
          map("v", "<leader>hr", function() gs.reset_hunk {vim.fn.line("."), vim.fn.line("v")} end)
          map("n", "<leader>hS", gs.stage_buffer)
          -- stage_hunk toggles the hunk under the cursor, including staged hunks.
          map("n", "<leader>hu", gs.stage_hunk)
          map("n", "<leader>hR", gs.reset_buffer)
          map("n", "<leader>hp", gs.preview_hunk)
          map("n", "<leader>hb", function() gs.blame_line{full=true} end)
          map("n", "<leader>tb", gs.toggle_current_line_blame)
          map("n", "<leader>hd", gs.diffthis)
          map("n", "<leader>hD", function() gs.diffthis("~") end)
          -- Preview only the current hunk inline; moving the cursor closes it.
          map("n", "<leader>td", gs.preview_hunk_inline)
        end
      })
    end,
  },
}

local options = {
  -- Lazy.nvim configuration options
  defaults = {
    lazy = true, -- Lazy-load plugins by default; eager plugins opt in explicitly
  },
  install = {
    missing = true, -- Auto-install missing plugins
  },
  performance = {
    rtp = {
      disabled_plugins = {
        "gzip",
        "netrw",
        "netrwPlugin",
        "tarPlugin",
        "tohtml",
        "tutor",
        "zipPlugin",
      },
    },
  },
}

return { plugins = plugins, options = options }
