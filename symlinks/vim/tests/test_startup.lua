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
      return vim.api.nvim_get_current_win() == original_window
        and vim.api.nvim_get_mode().mode == "n"
    end, "Escape must return to the previous window in normal mode", function()
      vim.api.nvim_set_current_win(terminal_window)
      await_state(function() return vim.api.nvim_get_mode().mode == "t" end,
        "Re-entering a terminal must restore terminal input", function()
          vim.api.nvim_set_current_win(original_window)
          await_state(function() return vim.api.nvim_get_mode().mode == "n" end,
            "Leaving a terminal must restore normal mode", function()
              vim.fn.jobstop(job)
              vim.cmd("qa!")
            end)
        end)
    end)
  end)
