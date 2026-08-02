-- Tokyo Night palette
hl.config({
    general = {
        gaps_in = 4,
        gaps_out = 8,
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
        blur = {
            enabled = false,
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
        render_unfocused_fps = 60,
    },

    ecosystem = {
        no_update_news = true,
        no_donation_nag = true,
    },

    xwayland = {
        force_zero_scaling = true,
    },
})

-- Remove gaps and borders for fullscreen windows
hl.workspace_rule({
    workspace = "f[1]",
    gaps_in = 0,
    gaps_out = 0,
    no_border = true,
    no_rounding = true,
})

hl.curve("subtle", { type = "bezier", points = { { 0.25, 0.1 }, { 0.25, 1.0 } } })
hl.curve("smooth", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.0 } } })

hl.animation({ leaf = "windows", enabled = true, speed = 3, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "smooth" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 3, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "border", enabled = true, speed = 8, bezier = "smooth" })
hl.animation({ leaf = "borderangle", enabled = true, speed = 60, bezier = "subtle", style = "loop" })
