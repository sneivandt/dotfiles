local mod = "SUPER"
local resizeOnBorder = require("conf.resize-on-border")

-- Launch
hl.bind(mod .. " + Return", hl.dsp.exec_cmd("alacritty"))
hl.bind(mod .. " + c", hl.dsp.exec_cmd("chatgpt"))
hl.bind(mod .. " + p", hl.dsp.exec_cmd("fuzzel"))
hl.bind(mod .. " + o", hl.dsp.exec_cmd("~/.config/hypr/scripts/choose-browser.sh"))
-- Use `test -x` rather than `[ -x ... ]`: Hyprland parses a leading bracket
-- group in an exec command as execution rules and strips it.
hl.bind(mod .. " + g", hl.dsp.exec_cmd('test -x "$HOME/.local/bin/github-copilot" && exec "$HOME/.local/bin/github-copilot"'))
hl.bind(mod .. " + v", hl.dsp.exec_cmd("~/.config/hypr/scripts/choose-editor.sh"))

-- Window management
hl.bind(mod .. " + q", hl.dsp.window.close())
hl.bind(mod .. " + f", hl.dsp.window.fullscreen({ mode = "maximized" }))
hl.bind(mod .. " + SHIFT + f", hl.dsp.window.fullscreen_state({ internal = 0, client = 2, action = "toggle" }))
hl.bind(mod .. " + j", hl.dsp.layout("cyclenext"))
hl.bind(mod .. " + k", hl.dsp.layout("cycleprev"))
hl.bind(mod .. " + SHIFT + j", hl.dsp.layout("swapnext"))
hl.bind(mod .. " + SHIFT + k", hl.dsp.layout("swapprev"))
hl.bind(mod .. " + h", hl.dsp.window.resize({ x = -20, y = 0, relative = true }))
hl.bind(mod .. " + l", hl.dsp.window.resize({ x = 20, y = 0, relative = true }))
hl.bind(mod .. " + t", function()
    hl.dispatch(hl.dsp.window.float({ action = "toggle" }))
    resizeOnBorder.update()
end)
hl.bind(mod .. " + comma", hl.dsp.layout("addmaster"))
hl.bind(mod .. " + period", hl.dsp.layout("removemaster"))

-- Lock screen
hl.bind(mod .. " + End", hl.dsp.exec_cmd("~/.config/hypr/scripts/lock-screen.sh"))

-- Screenshot (grim + slurp)
hl.bind(mod .. " + SHIFT + s", hl.dsp.exec_cmd("~/.config/hypr/scripts/screenshot.sh"))

-- Workspaces
for i = 1, 9 do
    hl.bind(mod .. " + " .. i, hl.dsp.focus({ workspace = i }))
    hl.bind(mod .. " + SHIFT + " .. i, hl.dsp.window.move({ workspace = i, follow = false }))
end

-- Workspace cycling
hl.bind(mod .. " + Tab", hl.dsp.focus({ workspace = "e+1" }))
hl.bind(mod .. " + SHIFT + Tab", hl.dsp.focus({ workspace = "e-1" }))

-- Mouse bindings
hl.bind(mod .. " + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind(mod .. " + mouse:273", hl.dsp.window.resize(), { mouse = true })
