---
name: hyprland-session
description: >
  Use for symlinks/config/hypr/ Lua configuration and helper scripts, Hypridle,
  Hyprlock, graphical-session user services, and desktop portal configuration.
  Not for Quickshell QML internals, Rust service resources, or host-wide triage.
---

# Hyprland Session

## Find the owner

| Change | Start with |
|---|---|
| compositor configuration and module loading | `symlinks/config/hypr/hyprland.lua`, `conf/` |
| bindings and launched commands | `conf/binds.lua`, `symlinks/config/hypr/scripts/` |
| idle and lock lifecycle | `hypridle.conf`, `hyprlock.conf`, `scripts/lock-screen.sh` |
| daemon lifetime and sandboxing | `symlinks/config/systemd/user/` |
| portal selection | `symlinks/config/xdg-desktop-portal/hyprland-portals.conf` |

Paths in the table without a prefix are relative to
[`symlinks/config/hypr/`](../../../symlinks/config/hypr/).
Read the current Lua API usage rather than translating examples written for
another Hyprland configuration format.

## Preserve session ownership

- UWSM owns session identity variables. Keep application-specific environment
  settings in `conf/env.lua`; do not override session identity there.
- Long-running daemons belong in the existing systemd user services. Preserve
  graphical-session lifetime, executable/config conditions, restart policy,
  and sandboxing instead of also launching a second instance from autostart.
- Trace a binding through its helper and service before changing it. Preserve
  cancellation, unavailable-tool behavior, and observable subprocess failures.
- Keep locking coordinated with idle/suspend behavior and the transient locker
  unit. Do not bypass the service boundary to work around its sandbox.
- Quote external paths and values, keep launch arguments separate where possible,
  and retain the existing script regression harness rather than requiring a live
  compositor for parsing or command-construction tests.

## Verify without disrupting the session

Use the affected [helper tests](../../../symlinks/config/hypr/scripts/tests/)
and the commands in [Application configuration](../../../docs/TESTING.md#application-configuration).
Inspect affected unit dependencies and conditions for service changes.
Fixture tests cannot prove native rendering, focus, locking, portals, or suspend.
Report that limit; live lock/suspend, workspace changes, service restarts, and
connectivity changes need authorization for those machine actions.

Use `desktop-shell` for Quickshell presentation and state, adding this skill
when its service/session integration also changes. Use `symlink-management`
for link selection and `system-files` for managed `/etc` fragments.
