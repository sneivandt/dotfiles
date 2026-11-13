hl.window_rule({
    name = "float-pavucontrol",
    match = { class = "^(pavucontrol)$" },
    float = true,
})

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
