local specs
package.preload["lazy"] = function()
  return {
    setup = function(plugins)
      specs = plugins
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
  if path == lazypath then
    return { type = "directory" }
  end
  return fs_stat(path)
end
dofile(vim.env.DIR .. "/symlinks/vim/lua/lazy-bootstrap.lua")
uv.fs_stat = fs_stat

local treesitter
for _, spec in ipairs(specs) do
  if spec[1] == "nvim-treesitter/nvim-treesitter" then
    treesitter = spec
    break
  end
end
assert(treesitter, "Tree-sitter plugin must be configured")
assert(treesitter.build == ":TSUpdate", "Plugin updates must update installed parsers")
treesitter.config()
assert(installed == nil, "Startup must not install parsers")
vim.cmd("TSInstallConfigured")
assert(installed and #installed == 18, "Explicit setup must install the configured parsers")
assert(vim.tbl_contains(installed, "rust"), "Configured parsers must include Rust")
assert(vim.tbl_contains(installed, "markdown_inline"), "Configured parsers must include Markdown injections")

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
