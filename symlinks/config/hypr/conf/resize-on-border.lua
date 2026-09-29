-- Border resizing is global, so follow the focused window rather than counting
-- workspace tiles. Ordinary tiled and floating windows remain resizable.

local M = {}

local enabled = nil

function M.update()
    local window = hl.get_active_window()
    -- The Lua window state is 0 = normal, 1 = maximized, 2 = fullscreen.
    local want = not (window and window.fullscreen == 1)
    if want == enabled then
        return
    end
    enabled = want
    hl.config({
        general = {
            resize_on_border = want,
            hover_icon_on_border = want,
        },
    })
end

hl.on("config.reloaded", function()
    enabled = nil
    M.update()
end)

for _, event in ipairs({
    "hyprland.start",
    "window.open",
    "window.close",
    "window.destroy",
    "window.fullscreen",
    "window.move_to_workspace",
    "window.active",
    "workspace.active",
    "monitor.focused",
}) do
    hl.on(event, M.update)
end

return M
