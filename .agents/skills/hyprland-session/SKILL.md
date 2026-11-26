---
name: hyprland-session
description: >
  Use for symlinks/config/hypr/ Lua configuration and helper scripts, Hypridle,
  Hyprlock, graphical-session user services, and desktop portal configuration.
  Not for Quickshell QML internals (use desktop-shell), Rust service resources,
  or host-wide triage.
---

# Hyprland Session

User-facing session, service, and shortcut documentation lives in
[Arch desktop](../../../docs/content/guides/arch-desktop.md).

## Non-obvious invariants

- Hyprland config under [`symlinks/config/hypr/`](../../../symlinks/config/hypr/)
  uses a Lua API (`hyprland.lua` + `conf/*.lua`), not native `.conf` syntax—
  follow existing Lua API usage.
- UWSM owns session identity variables; keep `symlinks/config/hypr/conf/env.lua`
  strictly application-specific and do not override session identity there.
- Long-running daemons belong in `symlinks/config/systemd/user/` with
  graphical-session lifetime, conditions, restart policy, and sandboxing—do not
  duplicate them in Hyprland autostart.
- Keep `hypridle.conf`, `hyprlock.conf`, and `scripts/lock-screen.sh`
  coordinated with the transient locker unit; do not bypass the service boundary
  to work around its sandbox.

## Verification

Run the affected tests in
[`.github/workflows/scripts/linux/hypr_script_tests/`](../../../.github/workflows/scripts/linux/hypr_script_tests/)
(see [Application configuration](../../../docs/content/development/testing.md#application-configuration)).
Offline fixtures do not prove live rendering, locking, portals, or suspend;
live lock/suspend, workspace switches, or service restarts require explicit
authorization.
