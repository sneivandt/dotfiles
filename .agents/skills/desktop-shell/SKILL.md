---
name: desktop-shell
description: >
  Use for the Quickshell QML/JavaScript UI and Python helper under
  symlinks/config/quickshell/: bars, menus/popups, media controls, tray,
  workspace layout, shared state, idle polling, and network actions. Not for
  Rust CLI output, shell bootstrap, or Hyprland/session configuration (use
  hyprland-session).
---

# Desktop Shell

Keep user-facing bar, menu, appearance, and reload guidance in
[Arch desktop](../../../docs/content/guides/arch-desktop.md), not Application settings.

## Find the owner

Source paths below are relative to
[`symlinks/config/quickshell/`](../../../symlinks/config/quickshell/), and test
paths are relative to
[`.github/workflows/scripts/linux/quickshell_tests/`](../../../.github/workflows/scripts/linux/quickshell_tests/).

| Change | Start with |
|---|---|
| screen instances, shared services, open-menu coordination | `shell.qml`, `qml/tst_ServiceLifecycle.qml` |
| per-monitor bar and active media player | `Bar.qml`, `MediaPlayer.js`, `qml/tst_MediaPlayer.qml` |
| workspaces and system tray | `WorkspaceGroup.qml`, `TrayGroup.qml`, `qml/tst_WorkspaceGroup.qml`, `qml/tst_TrayGroup.qml` |
| calendar, volume, and power menus | `CalendarContent.qml`, `VolumeSlider.qml`, `PowerMenu.qml`, `qml/tst_CalendarContent.qml`, `python/test_power_menu.py` |
| network menu, state, queued refreshes, and helper | `NetworkMenu.qml`, `NetworkState.qml`, `RefreshQueue.qml`, `network_helper.py`, `qml/tst_NetworkMenu.qml`, `qml/tst_RefreshQueue.qml`, `python/test_network_helper.py` |
| shared audio and market state | `AudioState.qml`, `MarketState.qml`, `qml/tst_AudioState.qml` |
| popup lifecycle, reload notice, theme, and shared controls | `ShellPopup.qml`, `ReloadNotice.qml`, `Theme.js`, `BarBlock.qml`, `MenuButton.qml`, `MenuIconButton.qml` |

## Preserve reactive behavior

- Reuse shared state objects instead of starting a poller for every screen or
  popup. Keep service logic out of presentation delegates.
- Keep expensive detail queries on-demand (for example, fetch Wi-Fi details
  only while `NetworkMenu` is open) rather than polling in the background while
  menus are closed; cover lifecycle guards in `qml/tst_ServiceLifecycle.qml`.
- Keep media controls tracking the actively playing player (`MediaPlayer.js`)
  rather than pinning the first registered player.
- Calendar and bar clocks receive the shared date from `shell.qml`. Keep
  calendar highlighting and month arithmetic in the selected local/UTC zone.
- Pass dependencies through declared properties; keep extracted visual components
  independent of the live compositor where practical.
- Preserve per-monitor workspace behavior, transient missing model entries, and
  null service objects. Derive geometry from state rather than adding timers to
  hide deferred-layout glitches.
- Reuse the popup's stable Wayland buffer and animation/focus lifecycle. Preserve
  Escape, outside-click dismissal, menu switching, screen bounds, and scrolling.
- Use theme tokens and existing controls. Keep accessible names, keyboard
  activation, focus indication, and disabled/busy behavior when changing controls.

## Keep actions explicit

- Use argument-vector processes, not shell interpolation of device names, SSIDs,
  window titles, or other external values.
- Keep the network helper's structured JSON protocol in sync with QML consumers.
  Preserve `ok`/error handling, subprocess timeouts, and escaped `nmcli` fields.
- Send credentials through the existing stdin path, not command arguments or
  logs. Use only synthetic credentials in fixtures; never expose real ones in
  screenshots. Clear transient credential state after use.
- Distinguish unavailable state from disconnected or empty state; show failures
  rather than claiming an action succeeded. Prevent overlapping actions and
  refresh after completion.

## Validate without disrupting the session

Use the offscreen QML and mocked Python commands in
[Desktop shell testing](../../../docs/content/development/testing.md#desktop-shell), choosing only
the affected suite. Cover long/empty labels, narrow screens, multi-monitor and
out-of-order model updates for layout changes.

Offscreen tests do not prove live Wayland rendering or focus behavior. Do not
switch real workspaces, change connectivity, or restart the shell just to test a
patch without permission. Editing a symlinked source may itself trigger reload;
keep changes coherent and avoid temporary broken configurations.
