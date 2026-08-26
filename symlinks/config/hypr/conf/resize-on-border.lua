-- Disable resize_on_border when the focused workspace's window fills it.
--
-- A window "fills" the workspace when it's fullscreen, or when it's the only
-- tiled window on the workspace (with no border/gaps via the f[1] workspace
-- rule). In those cases the border-resize cursor is misleading because there's
-- nothing to resize against.

local M = {}

local enabled = nil

local function workspaceFills()
    local ws = hl.get_active_workspace()
    if not ws then
        return false
    end

    local tiled = 0
    for _, w in ipairs(ws:get_windows() or {}) do
        if not w.hidden then
            -- fullscreen: 0 = none, 1 = maximize, 2 = fullscreen
            if w.fullscreen >= 2 then
                return true
            end
            if not w.floating then
                tiled = tiled + 1
            end
        end
    end

    return tiled == 1
end

-- Exported so binds that change floating state can re-evaluate; Hyprland emits
-- no event for a float toggle.
function M.update()
    local want = not workspaceFills()
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
