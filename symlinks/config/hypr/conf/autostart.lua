-- Long-running session daemons are managed as systemd user services.
-- See ~/.config/systemd/user/ for gammastep, hypridle, hyprpaper, mako, volume, waybar.
--
-- hl.exec_cmd runs its argument through `sh -c`, so no explicit wrapper is needed.
hl.on("hyprland.start", function()
    -- Hide waybar when a window goes fullscreen.
    hl.exec_cmd(
        'command -v python3 >/dev/null 2>&1 && [ -x "$HOME/.config/hypr/scripts/fullscreen-waybar.py" ] && exec "$HOME/.config/hypr/scripts/fullscreen-waybar.py"'
    )
end)
