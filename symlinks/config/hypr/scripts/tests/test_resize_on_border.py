"""Run the real Lua policy with compositor state mocked in memory."""

from pathlib import Path
import shutil
import subprocess
import unittest


LUA = shutil.which("lua") or shutil.which("lua5.4")
MODULE = Path(__file__).resolve().parents[2] / "conf" / "resize-on-border.lua"


@unittest.skipUnless(LUA, "Requires a Lua interpreter")
class BorderResizeTests(unittest.TestCase):
    def test_only_focused_maximized_windows_disable_border_resize(self):
        result = subprocess.run(
            [LUA, "-", str(MODULE)],
            input=r"""
local active, enabled
local callbacks = {}
local updates = 0
hl = {
    get_active_window = function() return active end,
    config = function(options)
        enabled = options.general.resize_on_border
        assert(options.general.hover_icon_on_border == enabled)
        updates = updates + 1
    end,
    on = function(event, callback) callbacks[event] = callback end,
}
local policy = dofile(arg[1])
local function check(window, event, expected)
    active = window
    callbacks[event]()
    assert(enabled == expected, event .. ": unexpected border-resize policy")
end
check(nil, "hyprland.start", true)
check({fullscreen=0, floating=false}, "window.active", true)
check({fullscreen=1, floating=false}, "window.fullscreen", false)
-- A floating window above a maximized window must regain ordinary resizing.
check({fullscreen=0, floating=true}, "window.active", true)
check({fullscreen=1, floating=true}, "window.fullscreen", false)
check({fullscreen=0, floating=true}, "window.fullscreen", true)
check({fullscreen=2, floating=false}, "window.fullscreen", true)
check({fullscreen=1, floating=false}, "window.active", false)
check(nil, "window.close", true)
check({fullscreen=1, floating=false}, "config.reloaded", false)
enabled = true -- Appearance config can reset the global option during reload.
check({fullscreen=1, floating=false}, "config.reloaded", false)
local before = updates
policy.update()
assert(updates == before, "Unchanged policy must not repeatedly update config")
""",
            text=True, capture_output=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
