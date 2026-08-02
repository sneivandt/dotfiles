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
