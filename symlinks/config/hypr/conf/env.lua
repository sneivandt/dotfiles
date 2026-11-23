-- Application-specific environment variables
-- Session identity vars are owned by uwsm and must not be overridden here.
-- UWSM exports these to systemd --user via ~/.config/uwsm/env-hyprland.d/session.sh;
-- keep them here as a fallback for direct Hyprland launches without overwriting
-- explicit inherited values.
local function set_default_env(name, value)
    local current = os.getenv(name)
    if current == nil or current == "" then
        hl.env(name, value)
    end
end

set_default_env("QT_QPA_PLATFORM", "wayland;xcb")
set_default_env("QT_WAYLAND_DISABLE_WINDOWDECORATION", "1")
set_default_env("GDK_BACKEND", "wayland,x11")
set_default_env("XCURSOR_THEME", "Adwaita")
set_default_env("XCURSOR_SIZE", "24")
