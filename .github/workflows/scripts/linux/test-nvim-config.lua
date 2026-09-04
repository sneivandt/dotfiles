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
