---
name: desktop-shell
description: >
  Use for the Quickshell QML/JavaScript UI and Python helper under
  symlinks/config/quickshell/: bars, popups, workspace layout, shared state,
  and network actions. Not for Rust CLI output, shell bootstrap, or unrelated
  application configuration.
---

# Desktop Shell

## Find the owner

Paths below are relative to
[`symlinks/config/quickshell/`](../../../symlinks/config/quickshell/).

| Change | Start with |
|---|---|
| screen instances, shared services, open-menu coordination | `shell.qml` |
| per-monitor bar and service bindings | `Bar.qml` |
| workspace visibility, title sizing | `WorkspaceGroup.qml` and `tests/tst_WorkspaceGroup.qml` |
| palette, typography, spacing, animation | `Theme.js` |
| popup geometry, focus, dismissal | `ShellPopup.qml` |
| reusable controls | `BarBlock.qml`, `MenuButton.qml`, `MenuIconButton.qml` |
| service queries and actions | `AudioState.qml`, `NetworkState.qml`, `MarketState.qml` |
| NetworkManager protocol and parsing | `network_helper.py`, `network_helper_test.py` |

## Preserve reactive behavior

- Reuse shared state objects instead of starting a poller for every screen or
  popup. Keep service logic out of presentation delegates.
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
[Desktop shell testing](../../../docs/TESTING.md#desktop-shell), choosing only
the affected suite. Cover long/empty labels, narrow screens, multi-monitor and
out-of-order model updates for layout changes.

Offscreen tests do not prove live Wayland rendering or focus behavior. Do not
switch real workspaces, change connectivity, or restart the shell just to test a
patch without permission. Editing a symlinked source may itself trigger reload;
keep changes coherent and avoid temporary broken configurations.
