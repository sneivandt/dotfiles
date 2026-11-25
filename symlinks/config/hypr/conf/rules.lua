hl.window_rule({
    name = "render-unfocused-chromium",
    match = { class = "^(chromium)$" },
    render_unfocused = true,
})

hl.window_rule({
    name = "suppress-maximize-chromium",
    match = { class = "^(chromium)$" },
    suppress_event = "maximize",
})

-- The GTK portal supplies its own frame and shadow around file dialogs.
hl.window_rule({
    name = "gtk-portal-dialog",
    match = { initial_class = "^xdg-desktop-portal-gtk$" },
    float = true,
    size = { "monitor_w*0.64", "monitor_h*0.70" },
    center = true,
    border_size = 0,
    no_shadow = true,
    animation = "popin",
})

-- The project picker draws its own frame inside the transparent window margins.
hl.window_rule({
    name = "project-root-picker-frame",
    match = { title = "^Select Project Root$", float = true },
    decorate = false,
})

-- Frosted glass blur for translucent shell, launcher, and notification layers.
hl.layer_rule({
    name = "blur-bar",
    match = { namespace = "^dotfiles-bar$" },
    blur = true,
    blur_popups = true,
    ignore_alpha = 0.2,
})

hl.layer_rule({
    name = "blur-osd",
    match = { namespace = "^dotfiles-osd$" },
    blur = true,
    ignore_alpha = 0.2,
})

hl.layer_rule({
    name = "blur-reload-notice",
    match = { namespace = "^quickshell-reload-notice$" },
    blur = true,
    ignore_alpha = 0.2,
})

hl.layer_rule({
    name = "blur-launcher",
    match = { namespace = "^launcher$" },
    blur = true,
    ignore_alpha = 0.2,
})

hl.layer_rule({
    name = "blur-notifications",
    match = { namespace = "^notifications$" },
    blur = true,
    ignore_alpha = 0.2,
})
