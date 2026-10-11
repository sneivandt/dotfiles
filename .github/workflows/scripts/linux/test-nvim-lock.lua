-- Run with the installed configuration after :Lazy! restore.
local config = require("lazy.core.config")
local lockpath = config.options.lockfile
local tracked = vim.env.DIR .. "/symlinks/vim/lazy-lock.json"
local before = table.concat(vim.fn.readfile(tracked), "\n")
local actual = table.concat(vim.fn.readfile(lockpath), "\n")
assert(actual == before, "Plugin setup changed the tracked lockfile")
local lock = vim.json.decode(before)
for name, plugin in pairs(config.plugins) do
  if not plugin.is_local then
    assert(lock[name], "Missing locked revision for " .. name)
    local commit = vim.fn.system({ "git", "-C", plugin.dir, "rev-parse", "HEAD" }):gsub("%s+$", "")
    assert(vim.v.shell_error == 0, "Cannot read installed revision for " .. name)
    assert(commit == lock[name].commit, "Installed revision differs from lockfile for " .. name)
  end
end
print("Installed Neovim plugins match the tracked lockfile")
