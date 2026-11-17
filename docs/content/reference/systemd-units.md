---
title: Systemd units
description: Enable, start, disable, or stop systemd user and system services, timers, and sockets while distinguishing running state from boot or login activation.
---

# Systemd units

Systemd manages services and other unit types, including timers and sockets.
A user unit belongs to the user's service manager; a system unit belongs to
the machine's manager. This task manages both persistent enablement and the
current running state, not the contents of the unit files themselves.

| Task detail | Value |
|---|---|
| Selector | `systemd` |
| Commands | `update` |
| Availability | Supported systemd platform with configured units, outside CI |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | [Home symlinks](home-symlinks.md). Waits for [system packages](system-packages.md) and [AUR packages](aur-packages.md) when unit readiness requires it. Waits for the [dotfiles launcher](dotfiles-launcher.md) when enabling the first-login user service. |
| Implementation | [system/systemd_units.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/systemd_units.rs) |

## What it does

The task starts and enables units marked enabled and stops and disables units
marked disabled. It inspects unit state after its ordering dependencies finish,
even if one failed. The first-login user service runs the launcher at
`~/.local/bin/dotfiles`, so enabling that service waits for launcher installation
when both tasks are selected.

It needs systemctl, is excluded in CI, and inside WSL requires systemd already running
or degraded. System units can require sudo. Without a user manager, including Arch
chroot provisioning, it creates per-user enablement links for the next real login
instead of running services inside the chroot. See [Unit configuration](#configuration).

Enabling a unit arranges activation through its install relationships, often
at a later boot or login. Starting a unit activates it now. A timer can be
active while its associated one-shot service is idle; that is not necessarily
a failure. The [systemctl reference](https://www.freedesktop.org/software/systemd/man/latest/systemctl.html)
explains unit states, enablement, and masking.

## Configuration

Edit
[conf/systemd-units.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/systemd-units.toml).

The following illustrative declarations assume that `backup.timer` and
`old-backup.timer` are user units you provide, and that the system already
provides `fstrim.timer`. Substitute the units you intend to manage; these
entries do not create unit files:

```toml title="conf/systemd-units.toml (example)"
[linux]
units = [
  "backup.timer",
  { name = "fstrim.timer", scope = "system" },
  { name = "old-backup.timer", scope = "user", enabled = false },
]
```

A string means an enabled user unit. A table requires `name` and `scope`; `scope` is
`user` or `system`, and `enabled` defaults to `true`. Include a supported suffix:
`.service`, `.timer`, `.socket`, `.target`, `.path`, `.mount`, `.automount` or `.swap`.
Use the canonical unit name. If the live manager reports an alias, Dotfiles
asks you to declare the backing unit.

With a live manager:

- `enabled = true` means persistently enabled and started. Runtime-only
  enablement is not sufficient. Static units have no install instructions and
  cannot satisfy this declaration. Dotfiles retries a start previously skipped
  by a condition so systemd can evaluate it again. Retrying a system unit can
  require sudo even if the condition still prevents it from starting.
- `enabled = false` means disabled and stopped, including stopping a static
  unit that has no enablement links.
- Disabling is not masking: another unit may start it later.

User unit files usually arrive through symlinks; packages may provide others.
An enabled first-login service (`dotfiles-first-login.service`) can stay idle
once the deferred VS Code extensions marker is gone. If the marker exists,
Dotfiles retries the idle service.
When the managed Thunar settings service has already run, Dotfiles checks its
two managed preferences and reruns the service if either differs.
System-scope changes can require sudo. Inside WSL, systemd must already be running or
degraded; changing `wsl.conf` cannot start it in the current session.

Enabled units can execute consequential actions, including scheduled cleanup.
Inspect their commands and drop-ins before enabling an unfamiliar unit.
To stop managing a service and also stop it running, first apply an explicit `enabled =
false` declaration. Deleting its entry alone does not disable it.

Without a user manager, the task prepares per-user enablement links for the next login
rather than starting services. Disabling a unit removes its matching enablement links
even if the unit file is gone. Enablement links still work after managed home links are
replaced with local copies during removal. Offline enablement honors unit-specific
drop-ins for `WantedBy` and `RequiredBy`. Templates, other install relationships, and
install directives in shared drop-ins require a live user manager; the task reports
these instead of partially enabling the unit. Conflicting declarations for the same
scope and unit are rejected.

See [Systemd units](#what-it-does) for when the task can run. Contributors changing
offline unit lookup or enablement parsing should start with the [systemd_unit.rs
implementation](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/systemd_unit.rs)
and its [regression
tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/tests/systemd_unit.rs).

For the illustrative `backup.timer` above, inspect both dimensions without
changing it. Replace the name with an existing user declaration on your machine:

```sh
systemctl --user is-enabled backup.timer
systemctl --user status backup.timer
```

For system-scope units, omit `--user` and use the declared system unit name.
A disabled or inactive unit can give a nonzero inspection status; compare it
with the desired declaration rather than treating every such status as a
failed installation. Unit startup failures need their service logs, not just
another enable attempt.

## Preview and apply

Preview the selected units' desired enablement and running state:

```sh
dotfiles update --only systemd --dry-run
```

Check unit names, user/system scope, and disabled declarations. Resolve missing
unit files or manager requirements before [applying the
change](task-reference.md#preview-and-apply). Review unit commands first,
especially timers that schedule cleanup. Afterward, use the enablement and
status queries above; offline user enablement must be checked again after
a real login.

## Removal

To stop a managed unit, first set `enabled = false` and apply the reviewed change while
the declaration is still selected. Deleting the entry alone does not disable or stop the
unit, and `dotfiles remove` does not reverse service state.
