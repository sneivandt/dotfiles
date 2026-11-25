-- Tokyo Night palette
hl.config({
    general = {
        gaps_in = 4,
        -- Top gap (4) combines with the bar's bottom inset (4) to match the 8px outer edges.
        gaps_out = { top = 4, right = 8, bottom = 8, left = 8 },
        border_size = 2,
        col = {
            active_border = { colors = { "rgb(7aa2f7)", "rgb(bb9af7)" }, angle = 45 },
            inactive_border = "rgb(414868)",
        },
        layout = "master",
        resize_on_border = true,
    },

    decoration = {
        rounding = 8,
        rounding_power = 3.5,
        dim_inactive = true,
        dim_strength = 0.08,
        blur = {
            enabled = true,
            size = 6,
            passes = 2,
            new_optimizations = true,
            vibrancy = 0.16,
            ignore_opacity = true,
        },
        shadow = {
            enabled = true,
            range = 12,
            render_power = 2,
            color = "rgba(00000040)",
            offset = { 0, 2 },
        },
    },

    animations = {
        enabled = true,
    },

    master = {
        new_status = "slave",
        mfact = 0.618,
    },

    misc = {
        disable_hyprland_logo = true,
        disable_splash_rendering = true,
        force_default_wallpaper = 0,
        render_unfocused_fps = 15,
    },

    ecosystem = {
        no_update_news = true,
        no_donation_nag = true,
    },

    -- Let legacy X11 windows follow the monitor scale as well.
    xwayland = {
        force_zero_scaling = false,
    },
})

-- Remove gaps and borders for single tiled (w[tv1]) and maximized (f[1]) windows.
for _, workspace in ipairs({ "w[tv1]", "f[1]" }) do
    hl.workspace_rule({
        workspace = workspace,
        gaps_in = 0,
        gaps_out = 0,
        no_border = true,
        no_rounding = true,
    })
end

hl.curve("smooth", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.0 } } })

hl.animation({ leaf = "windows", enabled = true, speed = 3, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "smooth" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 3, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "border", enabled = true, speed = 8, bezier = "smooth" })
hl.animation({ leaf = "borderangle", enabled = false })
