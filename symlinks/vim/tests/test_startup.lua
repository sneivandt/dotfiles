local function check(condition, message)
  if not condition then
    vim.api.nvim_err_writeln(message)
    vim.cmd("cquit")
  end
end

check(vim.v.errmsg == "", "Startup reported an error: " .. vim.v.errmsg)
check(vim.g.mapleader == ",", "init.vim must load the shared vimrc")
check(vim.g.test_bootstrap_loaded, "nvimrc must load the plugin bootstrap")
check(vim.fn.maparg(",t", "n") == ":terminal<CR>", "Terminal mapping must be loaded")
check(vim.fn.maparg("<C-e>", "n") == ":NvimTreeToggle<CR>", "Plugin mapping must be loaded")
check(vim.fn.maparg("<BS>", "n") == ":<C-U>TmuxNavigateLeft<CR>", "Neovim must override shared navigation")

if vim.fn.has("nvim-0.11") == 1 then
  check(vim.tbl_contains(vim.opt.completeopt:get(), "fuzzy"), "Fuzzy completion must be enabled")
end
if vim.fn.has("nvim-0.12") == 1 then
  check(vim.tbl_contains(vim.opt.completeopt:get(), "nearest"), "Nearby completion must be enabled")
  check(vim.fn.exists(":Undotree") == 2, "Built-in :Undotree command must be loaded")
  check(vim.fn.maparg(",u", "n") == ":Undotree<CR>", "Undo tree mapping must be loaded")

  -- Simulate lazy.nvim resetting runtimepath during setup, then verify
  -- :Undotree still loads lua/undotree.lua and toggles cleanly.
  local saved_rtp = vim.opt.rtp:get()
  local saved_preload = package.preload["lazy-bootstrap"]
  local reset_rtp = vim.tbl_filter(function(entry)
    return not entry:find("nvim%.undotree", 1)
  end, saved_rtp)
  vim.opt.rtp = reset_rtp
  package.loaded["lazy-bootstrap"] = nil
  package.loaded["undotree"] = nil
  vim.g.loaded_undotree_plugin = nil
  pcall(vim.api.nvim_del_user_command, "Undotree")
  package.preload["lazy-bootstrap"] = function()
    vim.opt.rtp = reset_rtp
    return {}
  end
  vim.cmd("source ~/.vim/nvimrc")
  package.preload["lazy-bootstrap"] = saved_preload

  local initial_win = vim.api.nvim_get_current_win()
  local ok_open, err_open = pcall(vim.cmd, "Undotree")
  check(ok_open, ":Undotree must open after lazy.nvim resets runtimepath: " .. tostring(err_open))
  check(#vim.api.nvim_list_wins() > 1, ":Undotree must open an undo tree window")
  vim.wait(0)
  local ok_close, err_close = pcall(vim.cmd, "Undotree")
  check(ok_close, ":Undotree must close cleanly: " .. tostring(err_close))
  vim.wait(0)
  check(#vim.api.nvim_list_wins() == 1 and vim.api.nvim_get_current_win() == initial_win,
    ":Undotree toggle must restore the original window")
  vim.opt.rtp = saved_rtp
end

-- Mode changes happen after commands return to the event loop. Poll for the
-- observable state with a deadline instead of assuming a fixed startup delay.
local function await_state(predicate, message, continuation)
  local deadline = (vim.uv or vim.loop).hrtime() + 2e9
  local function poll()
    if predicate() then
      continuation()
    else
      check((vim.uv or vim.loop).hrtime() < deadline,
        message .. " (mode=" .. vim.api.nvim_get_mode().mode .. ")")
      vim.defer_fn(poll, 10)
    end
  end
  vim.schedule(poll)
end

local original_window = vim.api.nvim_get_current_win()
vim.cmd("vnew")
local terminal_window = vim.api.nvim_get_current_win()
vim.cmd("terminal /bin/sh -c cat")
local job = vim.b.terminal_job_id
check(job > 0, "Fixture terminal must start")

await_state(function() return vim.api.nvim_get_mode().mode == "t" end,
  "Entering a terminal must start terminal input", function()
    vim.api.nvim_input("<Esc>")
    await_state(function()
      return vim.api.nvim_get_current_win() == terminal_window
        and vim.api.nvim_get_mode().mode == "nt"
    end, "Escape must keep the terminal focused in normal mode", function()
      vim.cmd("startinsert")
      await_state(function() return vim.api.nvim_get_mode().mode == "t" end,
        "Re-entering a terminal must restore terminal input", function()
          vim.api.nvim_input("<M-Esc>")
          await_state(function()
            return vim.api.nvim_get_current_win() == original_window
              and vim.api.nvim_get_mode().mode == "n"
          end, "Alt+Escape must return to the previous window in normal mode", function()
            vim.fn.jobstop(job)
            vim.cmd("qa!")
          end)
        end)
    end)
  end)
