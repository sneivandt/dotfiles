---
title: Systemd units
description: User and system service enablement and state.
---

# Systemd units

| Task | Selection |
|---|---|
| Selector | `systemd` |
| Commands | `update` |

## What it does

Enables/starts enabled declarations and disables/stops disabled ones. It waits for
system packages, AUR packages, symlinks, and permissions, then inspects the actual unit
state. Failures in that earlier work do not automatically block services that are
already available. This task never installs packages itself.

It needs systemctl, is excluded in CI, and inside WSL requires systemd already running
or degraded. System units can require sudo. Without a user manager, including Arch
chroot provisioning, it creates per-user enablement links for the next real login
instead of running services inside the chroot. See [Unit configuration](#configuration).

## Configuration

Edit
[conf/systemd-units.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/systemd-units.toml).

```toml title="conf/systemd-units.toml (excerpt)"
[linux]
units = ["clean-home-tmp.timer"]

[arch-desktop]
units = [
  { name = "NetworkManager.service", scope = "system" },
  { name = "dhcpcd.service", scope = "system", enabled = false },
  "quickshell.service",
]
```

A string means an enabled user unit. A table requires `name` and `scope`; `scope` is
`user` or `system`, and `enabled` defaults to `true`. Include a supported suffix:
`.service`, `.timer`, `.socket`, `.target`, `.path`, `.mount`, `.automount` or `.swap`.

With a live manager:

- `enabled = true` means persistently enabled and started. Runtime-only
  enablement is not sufficient.
- `enabled = false` means disabled and stopped, including stopping a static
  unit that has no enablement links.
- Disabling is not masking: another unit may start it later.

User unit files usually arrive through symlinks; packages may provide others.
System-scope changes can require sudo. Inside WSL, systemd must already be running or
degraded; changing `wsl.conf` cannot start it in the current session.

Some selected units perform [automatic
cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service).
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

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only systemd --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

To stop a managed unit, first set `enabled = false` and apply the reviewed change while
the declaration is still selected. Deleting the entry alone does not disable or stop the
unit, and `dotfiles remove` does not reverse service state.
