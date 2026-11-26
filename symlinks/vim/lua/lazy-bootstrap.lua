-- Bootstrap lazy.nvim plugin manager

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

-- Disable netrw before lazy.nvim / nvim-tree.lua initialize.
vim.g.loaded_netrw = 1
vim.g.loaded_netrwPlugin = 1

local ok, lazy = pcall(require, "lazy")
if not ok then
  bootstrap_error("failed to load lazy.nvim from " .. lazypath, lazy)
end
local config = require("lazy-config")
lazy.setup(config.plugins, config.options)
