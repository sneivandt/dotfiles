-- Border resizing is global, so disable it when the focused window fills its
-- workspace with no gaps (maximized f[1] or sole visible tiled w[tv1]).
-- Multi-tile layouts and floating windows remain resizable.

local M = {}

local enabled = nil

local function isSameWindow(a, b)
    if not a or not b then
        return false
    end
    if a == b then
        return true
    end
    return a.address ~= nil and a.address == b.address
end

local function isVisibleTiled(window)
    return not window.floating
        and window.mapped ~= false
        and not window.hidden
        and window.accepts_input ~= false
end

local function isSingleTiledWindow(window, closing)
    if window.floating or (window.fullscreen or 0) ~= 0 then
        return false
    end

    local workspace = window.workspace or (hl.get_active_workspace and hl.get_active_workspace())
    if not workspace or type(workspace.get_windows) ~= "function" then
        return false
    end

    local tiled = 0
    for _, candidate in ipairs(workspace:get_windows() or {}) do
        if not isSameWindow(candidate, closing) and isVisibleTiled(candidate) then
            tiled = tiled + 1
            if tiled > 1 then
                return false
            end
        end
    end

    return tiled == 1
end

local function update(closing)
    local window = hl.get_active_window()
    -- The Lua window state is 0 = normal, 1 = maximized, 2 = fullscreen.
    local want = not (window and (window.fullscreen == 1 or isSingleTiledWindow(window, closing)))
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

function M.update()
    update(nil)
end

hl.on("config.reloaded", function()
    enabled = nil
    update(nil)
end)

hl.on("window.close", function(closing)
    update(closing)
end)

for _, event in ipairs({
    "hyprland.start",
    "window.open",
    "window.destroy",
    "window.fullscreen",
    "window.minimize",
    "window.move_to_workspace",
    "window.pin",
    "window.active",
    "window.update_state",
    "workspace.active",
    "workspace.special_active",
    "monitor.focused",
}) do
    hl.on(event, function()
        update(nil)
    end)
end

return M
