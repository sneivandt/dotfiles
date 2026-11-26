"""Run the real Lua policy with compositor state mocked in memory."""

from pathlib import Path
import shutil
import subprocess
import unittest


LUA = shutil.which("lua") or shutil.which("lua5.4")
MODULE = Path(__file__).resolve().parents[5] / "symlinks/config/hypr/conf/resize-on-border.lua"


@unittest.skipUnless(LUA, "Requires a Lua interpreter")
class BorderResizeTests(unittest.TestCase):
    def test_disables_border_resize_for_maximized_and_single_tiled_windows(self):
        result = subprocess.run(
            [LUA, "-", str(MODULE)],
            input=r"""
local active, active_workspace, enabled
local callbacks = {}
local updates = 0

local function makeWorkspace(windows)
    return {
        get_windows = function() return windows end,
    }
end

local function makeWindow(opts)
    local w = {
        address = opts.address,
        fullscreen = opts.fullscreen or 0,
        floating = opts.floating or false,
        mapped = opts.mapped,
        hidden = opts.hidden or false,
        accepts_input = opts.accepts_input,
        workspace = opts.workspace,
    }
    if w.mapped == nil then w.mapped = true end
    if w.accepts_input == nil then w.accepts_input = true end
    return w
end

hl = {
    get_active_window = function() return active end,
    get_active_workspace = function() return active_workspace end,
    config = function(options)
        enabled = options.general.resize_on_border
        assert(options.general.hover_icon_on_border == enabled)
        updates = updates + 1
    end,
    on = function(event, callback) callbacks[event] = callback end,
}

local policy = dofile(arg[1])

local function check(window, event, expected, event_arg)
    active = window
    active_workspace = window and window.workspace or nil
    callbacks[event](event_arg)
    assert(enabled == expected, event .. ": expected " .. tostring(expected) .. ", got " .. tostring(enabled))
end

-- Empty workspace keeps default enabled state.
check(nil, "hyprland.start", true)

-- Single tiled window filling the workspace (w[tv1]) disables border resize.
local ws_single = makeWorkspace({})
local single_tiled = makeWindow({ address = "0x1", workspace = ws_single })
ws_single.get_windows = function() return { single_tiled } end
check(single_tiled, "window.open", false, single_tiled)

-- Adding a second visible tiled window restores border resize.
local second_tiled = makeWindow({ address = "0x2", workspace = ws_single })
ws_single.get_windows = function() return { single_tiled, second_tiled } end
check(second_tiled, "window.open", true, second_tiled)

-- Maximizing one of two tiled windows (f[1]) disables border resize.
second_tiled.fullscreen = 1
check(second_tiled, "window.fullscreen", false, second_tiled)

-- A floating window focused above a maximized or single tiled window remains resizable.
local floating = makeWindow({ address = "0x3", floating = true, workspace = ws_single })
ws_single.get_windows = function() return { single_tiled, floating } end
check(floating, "window.active", true, floating)
floating.fullscreen = 1
check(floating, "window.fullscreen", false, floating)
floating.fullscreen = 0
check(floating, "window.fullscreen", true, floating)

-- Focusing the single tiled window behind a floating window disables border resize again.
check(single_tiled, "window.active", false, single_tiled)

-- Floating that single tiled window re-enables border resize via window.update_state.
single_tiled.floating = true
check(single_tiled, "window.update_state", true, single_tiled)
single_tiled.floating = false
check(single_tiled, "window.update_state", false, single_tiled)

-- Fullscreen mode 2 preserves prior behavior.
second_tiled.fullscreen = 2
ws_single.get_windows = function() return { single_tiled, second_tiled } end
check(second_tiled, "window.fullscreen", true, second_tiled)
second_tiled.fullscreen = 0

-- Closing an unfocused second tiled window disables border resize immediately on window.close.
check(single_tiled, "window.active", true, single_tiled)
check(single_tiled, "window.close", false, second_tiled)

-- Unmapped, hidden, or inactive-group tiled peers do not count as a second visible tile.
local unmapped_peer = makeWindow({ address = "0x4", mapped = false, workspace = ws_single })
local hidden_peer = makeWindow({ address = "0x5", hidden = true, workspace = ws_single })
local inactive_tab = makeWindow({ address = "0x6", accepts_input = false, workspace = ws_single })
ws_single.get_windows = function() return { single_tiled, unmapped_peer, hidden_peer, inactive_tab } end
check(single_tiled, "window.destroy", false)

-- Moving a second tiled window off the workspace disables border resize.
local ws_other = makeWorkspace({ second_tiled })
second_tiled.workspace = ws_other
ws_single.get_windows = function() return { single_tiled, second_tiled } end
check(single_tiled, "window.active", true, single_tiled)
ws_single.get_windows = function() return { single_tiled } end
check(single_tiled, "window.move_to_workspace", false, second_tiled)

-- Closing the last window on the workspace restores the default enabled state.
check(single_tiled, "window.close", true, single_tiled)
check(nil, "window.active", true)

-- Reload resets cached state even if appearance.lua turned the global option back on.
enabled = true
check(single_tiled, "config.reloaded", false)
local before = updates
policy.update()
assert(updates == before, "Unchanged policy must not repeatedly update config")
""",
            text=True, capture_output=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
