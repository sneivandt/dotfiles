---
title: Application configuration
nav_order: 6
---

# Application settings

This is the map for changing the managed shell and Arch desktop experience.
Edit tracked sources under `symlinks/`, not installed copies or generated
files. Links may make this checkout live application configuration, so some
edits take effect immediately.

To change **which machines receive a file**, use
[Configuration](CONFIGURATION.md#category-sections). To change **what the file
does**, start here.

## Shell application configuration

| Setting | Source |
|---|---|
| Shared shell environment and pager defaults | [`config/shell/profile.sh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/shell/profile.sh) |
| Zsh completion | [`config/zsh/completion.zsh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/zsh/completion.zsh) |
| Git aliases | [`config/git/aliases`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/git/aliases) |
| Download naming and timestamping | [`config/wgetrc`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/wgetrc) |

### Environment and paging

The shared profile keeps nonempty supplied `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`,
`XDG_DATA_HOME`, `TERMINFO` and `TERMINFO_DIRS` values. Unset **or empty** values
use home-directory defaults. The default terminal search path also retains
the system terminfo database.

`LESS` is different: a supplied value, including an empty one, is preserved.
The default is `-imRj8`; add `-X` if you want pager output left in terminal
scrollback.

### Completion

Zsh attempts approximate completion only after a repeated Tab with unchanged
input, cursor position and history entry. A first Tab does not immediately
fall back to approximate matches.

Dotfiles command completion comes from the CLI's
[Shell completions task](TASKS.md#shell-completions), not a separate hand-edited
selector list.

### Git shortcuts

- `git gr <pattern>` searches case-insensitively with line numbers. Color is
  automatic for terminal output, not forced into redirected output.
- `git contributors` summarizes `HEAD`. Pass a revision range such as
  `main..HEAD` to select different commits.

Alias definitions belong in the linked Git files. Desired global key/value
overrides belong in [Git settings](CONFIGURATION.md#git-settings).

### Downloads

Wget preserves URL filenames and uses timestamping to reuse unchanged
downloads. Request these behaviors explicitly when needed:

| Flag | Use it for |
|---|---|
| `--adjust-extension` | Web-page filename extensions |
| `--trust-server-names` | Redirected filenames |
| `--content-disposition` | Names supplied by response headers |

Extension adjustment can prevent timestamping from recognizing a file
previously downloaded from the same URL.

## Desktop application configuration

The Arch desktop uses Hyprland and a Quickshell bar. These settings belong to
the desktop profile; Windows application integration is covered in
[Windows](WINDOWS.md).

### Arch desktop appearance

| Change | Start here |
|---|---|
| Colors, typography, spacing and motion | [`Theme.js`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/quickshell/Theme.js) |
| Popup placement, focus, dismissal and scrolling | [`ShellPopup.qml`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/quickshell/ShellPopup.qml) |
| Bar contents and layout | [`Bar.qml`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/quickshell/Bar.qml) |
| Notification spacing | [`mako/config`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/mako/config) |
| Window and application shortcuts | [`binds.lua`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/hypr/conf/binds.lua) |

Only one bar popup opens at a time across monitors. The bar reserves space for
tiled windows, sits below floating windows and hides on fullscreen workspaces.
Mako margins align notifications with the tiled frame without adding the bar
offset twice.

**Tray behavior.** GitHub Copilot and ChatGPT use monochrome brand icons in
the regular tray, visible only while the applications expose tray items.
Other applications retain their own icons. Context menus, middle-click and
scrolling remain available.

Copilot's left click goes through
[`launch-github.sh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/hypr/scripts/launch-github.sh) to invoke
the app's **Open GitHub Copilot** action; ordinary tray activation does not
restore that window. ChatGPT uses normal activation. Copilot launcher failures
appear in its tooltip and log. The brand SVGs carry Font Awesome Free/CC BY 4.0
attribution in [`icons/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/quickshell/icons).

**Menus.** The volume menu selects the default output and allows amplification
to 150%, with 100% marked. NetworkManager handles saved networks and Wi-Fi
password entry; enterprise setup stays in the connection editor. Logout,
restart and shutdown require confirmation; lock is immediate.

Saved Wi-Fi profile details are cached for up to 60 seconds; live adapter,
connection and signal state are still polled. Connection actions and profile
inventory changes invalidate that cache. Market refreshes keep each symbol's
last successful quote and timestamp on failure, marking stale data rather
than hiding every cached quote because one request failed.

### Reloading changes

Quickshell watches linked sources and keeps the last valid shell when a reload
fails. Editing this checkout can therefore reload the live UI without running
dotfiles again.

For deliberate manual reloads, Hyprland uses `hyprctl reload config-only`
and `hyprctl configerrors`; mako uses `makoctl reload`. Fuzzel reads its
configuration on launch. Existing GTK applications may need reopening.

Keep Hyprlock's simple background and disabled animations while its
[documented DPMS workaround](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/hypr/hyprlock.conf) is needed.

### Screen locking and suspend

[`hypridle.conf`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/hypr/hypridle.conf) requests locking after
**10 idle minutes** and turns displays off after **15**. Idle inhibitors can
postpone these actions. Turning off displays is not suspending the machine.

Before sleep, Hypridle requests a session lock. `inhibit_sleep = 3` waits for
the compositor's lock notification because automatic detection cannot see
Hyprlock behind the shared wrapper. This needs Hypridle/compositor support.

The delay is bounded by logind's `InhibitDelayMaxSec`: a failed locker does
**not** guarantee suspend will be prevented. Resume turns displays on but
does not unlock the session.

### Hyprland media keys

Bindings use standard `XF86` symbols, not physical F-key positions. Keyboard
Fn mode determines which symbols are sent. Fallback shortcuts are:

| Shortcut | Action |
|---|---|
| Super + F1 | Toggle output mute |
| Super + F2 / F3 | Lower / raise output volume by 5% |
| Super + F4 | Toggle microphone mute |
| Super + F5 / F6 | Lower / raise display brightness by 5% |

Volume follows the default output through `pactl` and can exceed 100%.
Playback uses `playerctl`. Dedicated media keys also work while locked;
fallback shortcuts do not.

[`brightness.sh`](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/hypr/scripts/brightness.sh) targets display
backlights, never keyboard LEDs, and keeps brightness above zero. With no
backlight it does nothing. External monitors usually need separate DDC/CI
configuration unless their driver exposes a backlight.
