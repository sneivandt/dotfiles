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

Sources live in [`symlinks/config/quickshell/`](../../../symlinks/config/quickshell/),
tests in [`tests/desktop/quickshell/`](../../../tests/desktop/quickshell/),
and user documentation in [Arch desktop](../../../docs/content/guides/arch-desktop.md)
(not Application settings).

## Non-obvious invariants

- Reuse shared state objects in `shell.qml` rather than spawning per-screen or
  per-popup pollers. Keep expensive detail queries on-demand (e.g., Wi-Fi details
  only while `NetworkMenu` is open, market data only while `MarketMenu` is open)
  and guard lifecycle behavior in `tests/desktop/quickshell/qml/tst_ServiceLifecycle.qml`.
- Keep media controls tracking the actively playing player (`MediaPlayer.js`),
  not the first registered player.
- Calendar and bar clocks share `shell.qml`'s date; keep highlighting and month
  arithmetic in the selected local/UTC zone.
- Reuse `ShellPopup.qml`'s stable Wayland buffer and animation/focus lifecycle
  (Escape, outside-click dismissal, menu switching, screen bounds, scrolling).
  Derive geometry from state rather than timers for deferred layout.
- Keep `network_helper.py`'s JSON protocol (`ok`/error, timeouts, escaped
  `nmcli` fields) in sync with QML consumers. Pass credentials only via stdin,
  never argv or logs, and clear transient credential state after use.
- Distinguish unavailable state from disconnected/empty state; prevent
  overlapping network actions and refresh after completion.

## Validation

Run the affected offscreen QML or mocked Python suite from
[Desktop shell testing](../../../docs/content/development/testing.md#desktop-shell).
Offscreen tests do not prove live Wayland rendering or focus, and editing
symlinked QML sources can trigger a live reload—keep edits coherent.
