-- Bootstrap lazy.nvim plugin manager
-- This file auto-installs lazy.nvim and loads plugin configurations

local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
local uv = vim.uv or vim.loop
local function bootstrap_error(message, output)
  vim.api.nvim_err_writeln("lazy.nvim bootstrap failed: " .. message)
  if output and output ~= "" then
    vim.api.nvim_err_writeln(output)
  end
  error("lazy.nvim bootstrap failed", 0)
end

local function manager_ready(path)
  for _, file in ipairs({ "/lua/lazy/init.lua", "/lua/lazy/core/config.lua" }) do
    local stat, stat_error, stat_code = uv.fs_stat(path .. file)
    if not stat and stat_code ~= "ENOENT" and stat_code ~= "ENOTDIR" then
      bootstrap_error("cannot inspect " .. path .. file, stat_error)
    end
    if not stat or stat.type ~= "file" then
      return false
    end
  end
  return true
end

if not manager_ready(lazypath) then
  -- Pin to a specific commit for security and reproducibility
  -- Keep this revision aligned with lazy.nvim in lazy-lock.json
  local lazy_commit = "306a05526ada86a7b30af95c5cc81ffba93fef97"

  if vim.fn.executable("git") ~= 1 then
    bootstrap_error("git is required to install lazy.nvim")
  end

  local function mkdir(path)
    local created, message, code = uv.fs_mkdir(path, 448)
    if not created and code == "ENOENT" then
      mkdir(vim.fn.fnamemodify(path, ":h"))
      created, message, code = uv.fs_mkdir(path, 448)
    end
    if not created then
      local stat = uv.fs_stat(path)
      if code ~= "EEXIST" or not stat or stat.type ~= "directory" then
        bootstrap_error("cannot create " .. path, message)
      end
    end
  end
  mkdir(vim.fn.fnamemodify(lazypath, ":h"))
  local staging, staging_error = uv.fs_mkdtemp(lazypath .. ".bootstrap-XXXXXX")
  if not staging then
    bootstrap_error("cannot create staging directory", staging_error)
  end

  local lockpath = lazypath .. ".bootstrap-lock"
  local owner = "owner-" .. uv.os_getpid() .. "-" .. vim.fn.fnamemodify(staging, ":t")
  local locked = false
  local ok, failure = pcall(function()
    local function git(args, message)
      local output = vim.fn.system(args)
      if vim.v.shell_error ~= 0 then
        error(message .. "\n" .. output, 0)
      end
    end
    git({
      "git", "clone", "--filter=blob:none",
      "https://github.com/folke/lazy.nvim.git", staging,
    }, "git clone failed")
    git({ "git", "-C", staging, "checkout", lazy_commit },
      "git checkout " .. lazy_commit .. " failed")
    if not manager_ready(staging) then
      error("checkout is missing lazy.nvim runtime files", 0)
    end

    -- Publish ownership with the lock, so a crash cannot leave an ownerless lock.
    local candidate, candidate_error = uv.fs_mkdtemp(staging .. "/.bootstrap-lock-XXXXXX")
    if not candidate then
      error("cannot prepare bootstrap lock: " .. tostring(candidate_error), 0)
    end
    local descriptor, owner_error = uv.fs_open(candidate .. "/" .. owner, "wx", 384)
    if not descriptor then
      error("cannot record bootstrap lock owner: " .. tostring(owner_error), 0)
    end
    uv.fs_close(descriptor)

    -- Clone without a shared lock; only recovery/publication needs serialization.
    local lock_error
    local acquired = vim.wait(5000, function()
      if manager_ready(lazypath) then
        return true
      end
      locked, lock_error = uv.fs_rename(candidate, lockpath)
      if locked then
        return true
      end
      local entries = uv.fs_scandir(lockpath)
      if entries then
        local previous, kind = uv.fs_scandir_next(entries)
        if not previous then
          uv.fs_rmdir(lockpath)
        elseif kind == "file" and not uv.fs_scandir_next(entries) then
          local pid = tonumber(previous:match("^owner%-(%d+)%-"))
          if pid then
            local alive, _, code = uv.kill(pid, 0)
            if not alive and code == "ESRCH" then
              -- The unique filename cannot unlink a successor's ownership.
              -- rmdir cannot remove an atomically published, nonempty lock.
              uv.fs_unlink(lockpath .. "/" .. previous)
              uv.fs_rmdir(lockpath)
            end
          end
        end
      end
      return false
    end, 20)
    if not acquired then
      error("cannot acquire " .. lockpath .. ": " .. tostring(lock_error)
        .. ". Another bootstrap may still own it; retry after it exits.", 0)
    end
    if manager_ready(lazypath) then
      return
    end

    local existing, stat_error, stat_code = uv.fs_lstat(lazypath)
    if not existing and stat_code ~= "ENOENT" then
      error("cannot inspect " .. lazypath .. ": " .. tostring(stat_error), 0)
    end
    if existing then
      if existing.type ~= "directory" then
        error("refusing to replace non-directory " .. lazypath, 0)
      end
      local backup = staging .. ".interrupted"
      local moved, move_error = uv.fs_rename(lazypath, backup)
      if not moved then
        error("cannot preserve incomplete installation: " .. tostring(move_error), 0)
      end
      vim.api.nvim_echo({ { "Preserved incomplete lazy.nvim at " .. backup, "WarningMsg" } }, true, {})
    end
    local published, publish_error = uv.fs_rename(staging, lazypath)
    if not published then
      error("cannot publish lazy.nvim: " .. tostring(publish_error), 0)
    end
  end)
  vim.fn.delete(staging, "rf")
  if locked then
    uv.fs_unlink(lockpath .. "/" .. owner)
    uv.fs_rmdir(lockpath)
  end
  if not ok then
    bootstrap_error(tostring(failure))
  end
end
vim.opt.rtp:prepend(lazypath)

-- Configure lazy.nvim with plugins
local ok, lazy = pcall(require, "lazy")
if not ok then
  bootstrap_error("failed to load lazy.nvim from " .. lazypath, lazy)
end

lazy.setup({
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
    end,
  },

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
          lualine_c = { { "tabs", mode = 1, path = 3, max_length = vim.o.columns } },
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
          "python", "javascript", "typescript", "lua", "vim", "vimdoc",
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
