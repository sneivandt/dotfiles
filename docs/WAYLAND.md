# Wayland Session

This page describes how the graphical session is started on Arch Linux desktop
systems and how to fall back to the legacy Xorg + xmonad setup.

## Default session: Hyprland (Wayland)

When you log in on **tty1**, `~/.zprofile` runs the session chooser script
`~/.local/bin/start-session`, which tries Hyprland first:

1. `zprofile` checks that neither `$DISPLAY` nor `$WAYLAND_DISPLAY` is set and
   that `$XDG_VTNR` equals `1`, then calls `exec ~/.local/bin/start-session`.
2. `start-session` runs `dbus-run-session Hyprland`.
3. If Hyprland starts and exits with a non-zero status (indicating an error or
   failure), `start-session` automatically falls back to `startx` (Xorg + xmonad).

The existing X11 configuration files (`~/.xinitrc`, `xsession.target`, and all
dependent services) are unchanged and remain fully functional.

A parallel systemd user target, `wayland-session.target`, is provided alongside
`xsession.target` for future user services that need to run inside a Wayland
session. It binds to `graphical-session.target` in the same way as
`xsession.target`.

## Forcing Xorg fallback

### For a single login

Set `FORCE_X11=1` before your shell reads `zprofile`. The easiest way is to
switch to **tty2** and log in there, or add the variable to a host-local file
sourced before `zprofile`:

```sh
FORCE_X11=1 zsh -l
```

### Persistently

Export the variable from `~/.zshenv` (or any file sourced before `~/.zprofile`)
on the target machine:

```sh
# ~/.zshenv (host-local override — not tracked by dotfiles)
export FORCE_X11=1
```

Alternatively, use the `PREFERRED_SESSION` variable:

```sh
export PREFERRED_SESSION=x11
```

Either variable causes `start-session` to invoke `startx` directly, skipping
the Wayland path entirely.

## Troubleshooting Hyprland failures

### Hyprland exits immediately

1. Switch to another tty (e.g. **Ctrl-Alt-F2**) and log in.
2. Force Xorg for the current shell:

   ```sh
   export FORCE_X11=1
   ```

3. Start an Xorg session:

   ```sh
   startx
   ```

4. Review the Hyprland log to diagnose the problem:

   ```sh
   cat ~/.local/share/hyprland/hyprland.log
   journalctl --user -b | grep -i hypr
   ```

### Missing portal or screen-sharing issues

Ensure the required packages are installed:

```sh
pacman -Q hyprland xdg-desktop-portal xdg-desktop-portal-hyprland xorg-xwayland
```

The `xdg-desktop-portal-hyprland` package provides screen capture, file
pickers, and other portal functionality for apps running inside the Hyprland
session.

### X11 applications look wrong under Wayland

Most X11 applications run transparently through **XWayland** (provided by the
`xorg-xwayland` package). If a specific application behaves unexpectedly, try
forcing it to use the active XWayland display explicitly (check the current
value with `echo $DISPLAY` from inside the Wayland session):

```sh
DISPLAY=:1 <application>
```

## See Also

- [Configuration Reference](CONFIGURATION.md) — packages and symlinks
- [Troubleshooting](TROUBLESHOOTING.md) — general installer issues
- [Profile System](PROFILES.md) — choosing between base and desktop profiles
