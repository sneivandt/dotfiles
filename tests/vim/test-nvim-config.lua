local specs, lazy_opts
package.preload["lazy"] = function()
  return {
    setup = function(plugins, opts)
      specs = plugins
      lazy_opts = opts
    end,
  }
end

local installed
package.preload["nvim-treesitter"] = function()
  return {
    install = function(parsers)
      installed = parsers
      return {
        wait = function()
          error("Parser installation must not block the editor")
        end,
      }
    end,
  }
end

local uv = vim.uv or vim.loop
local fs_stat = uv.fs_stat
local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
uv.fs_stat = function(path)
  if path == lazypath .. "/lua/lazy/init.lua"
    or path == lazypath .. "/lua/lazy/core/config.lua" then
    return { type = "file" }
  end
  return fs_stat(path)
end
dofile(vim.env.DIR .. "/symlinks/vim/lua/lazy-bootstrap.lua")
uv.fs_stat = fs_stat

assert(lazy_opts and lazy_opts.defaults and lazy_opts.defaults.lazy == true,
  "lazy.nvim should default to lazy-loading plugins")
local specs_by_name = {}
for _, spec in ipairs(specs) do
  specs_by_name[spec[1]] = spec
end
for _, removed in ipairs({
  "nanotech/jellybeans.vim",
  "morhetz/gruvbox",
  "neovimhaskell/haskell-vim",
  "pangloss/vim-javascript",
  "rust-lang/rust.vim",
}) do
  assert(specs_by_name[removed] == nil, "Unused plugin should stay removed: " .. removed)
end
for _, eager in ipairs({
  "folke/tokyonight.nvim",
  "nvim-lualine/lualine.nvim",
  "nvim-treesitter/nvim-treesitter",
}) do
  assert(specs_by_name[eager] and specs_by_name[eager].lazy == false,
    "Startup plugin must opt into eager loading: " .. eager)
end
assert(specs_by_name["nvim-tree/nvim-tree.lua"].cmd ~= nil, "nvim-tree should lazy-load on commands")
assert(specs_by_name["tpope/vim-fugitive"].cmd ~= nil, "vim-fugitive should lazy-load on commands")
assert(specs_by_name["junegunn/fzf.vim"].cmd ~= nil, "fzf.vim should lazy-load on commands")
assert(specs_by_name["junegunn/fzf.vim"].dependencies ~= nil, "fzf.vim must declare junegunn/fzf dependency")

local treesitter = specs_by_name["nvim-treesitter/nvim-treesitter"]
assert(treesitter, "Tree-sitter plugin must be configured")
assert(treesitter.build == ":TSUpdate", "Plugin updates must update installed parsers")
treesitter.config()
assert(installed == nil, "Startup must not install parsers")
vim.cmd("TSInstallConfigured")
assert(installed and #installed == 18, "Explicit setup must install the configured parsers")
assert(vim.tbl_contains(installed, "rust"), "Configured parsers must include Rust")
assert(vim.tbl_contains(installed, "markdown_inline"), "Configured parsers must include Markdown injections")

local ts_patterns = {}
for _, autocmd in ipairs(vim.api.nvim_get_autocmds({ group = "TreesitterConfig", event = "FileType" })) do
  ts_patterns[autocmd.pattern] = true
end
assert(ts_patterns["help"], "Tree-sitter FileType autocmd must match Neovim's help filetype")

local ibl_spec
for _, spec in ipairs(specs) do
  if spec[1] == "lukas-reineke/indent-blankline.nvim" then
    ibl_spec = spec
    break
  end
end
assert(ibl_spec, "indent-blankline.nvim must be configured")
local ibl_options
package.loaded.ibl = {
  setup = function(opts) ibl_options = opts end,
}
ibl_spec.config()
assert(ibl_options and ibl_options.exclude and ibl_options.exclude.filetypes, "ibl must configure excluded filetypes")
assert(vim.tbl_contains(ibl_options.exclude.filetypes, "NvimTree"), "ibl must exclude NvimTree")
assert(vim.tbl_contains(ibl_options.exclude.filetypes, "help"), "ibl must exclude help")
assert(vim.tbl_contains(ibl_options.exclude.filetypes, "lazy"), "ibl must exclude lazy")
assert(not vim.tbl_contains(ibl_options.exclude.filetypes, "neo-tree"), "ibl should not list unused neo-tree")

-- Exercise the actual buffer mappings without installing plugins or touching Git.
local gitsigns
for _, spec in ipairs(specs) do
  if spec[1] == "lewis6991/gitsigns.nvim" then
    gitsigns = spec
    break
  end
end
assert(gitsigns, "Gitsigns must be configured")
local options
local calls = {}
local function action(name)
  return function(...) calls[#calls + 1] = { name, ... } end
end
package.loaded.gitsigns = {
  setup = function(opts) options = opts end,
  nav_hunk = action("navigate"),
  stage_hunk = action("stage"),
  reset_hunk = action("reset"),
  stage_buffer = action("stage_buffer"),
  reset_buffer = action("reset_buffer"),
  preview_hunk = action("preview"),
  preview_hunk_inline = action("inline"),
  blame_line = action("blame"),
  toggle_current_line_blame = action("toggle_blame"),
  diffthis = action("diff"),
}
gitsigns.config()
vim.g.mapleader = ","
options.on_attach(vim.api.nvim_get_current_buf())
local function mapping(lhs)
  local map = vim.fn.maparg(lhs, "n", false, true)
  assert(map.buffer == 1 and type(map.callback) == "function", "Missing buffer mapping: " .. lhs)
  return map.callback
end
for _, case in ipairs({ { "]c", "next" }, { "[c", "prev" } }) do
  local callback = mapping(case[1])
  assert(callback() == "<Ignore>")
  assert(vim.wait(1000, function() return #calls > 0 end), "Navigation was not scheduled")
  assert(vim.deep_equal(table.remove(calls), { "navigate", case[2] }))
  vim.wo.diff = true
  assert(callback() == case[1], "Diff windows must retain native navigation")
  vim.wait(20, function() return #calls > 0 end)
  assert(#calls == 0, "Diff navigation must not invoke Gitsigns")
  vim.wo.diff = false
end
mapping(",hu")()
assert(vim.deep_equal(table.remove(calls), { "stage" }))
mapping(",td")()
assert(vim.deep_equal(table.remove(calls), { "inline" }))
print("Gitsigns mappings passed")
