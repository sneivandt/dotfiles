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
| [Runs after](task-reference.md#scheduling-model) | [Home symlinks](home-symlinks.md); [System packages](system-packages.md) and [AUR packages](aur-packages.md) (conditional); [Dotfiles launcher](dotfiles-launcher.md) (first-login unit) |
| Implementation | [system/systemd_units.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/systemd_units.rs) |

## What it does

Enables units marked `enabled = true` and stops and disables units marked
`enabled = false`. Enabled units start immediately unless marked `graphical = true`,
in which case they wait for an active user graphical session. The task checks unit
state once its ordering dependencies finish, even if one of those failed. Because the
first-login user service invokes `~/.local/bin/dotfiles`, enabling it waits for
launcher installation whenever both tasks are selected.

This task requires `systemctl`, is skipped in CI, and in WSL requires systemd to
already be running or degraded. An offline WSL manager skips the task, while a failed
status query reports an error rather than assuming systemd is offline. System units may
require `sudo`. Without a running user manager, Dotfiles creates per-user enablement
links for the next login; during Arch chroot provisioning, it also prepares system-unit
links for the next boot without querying, reloading, or starting a manager inside the
chroot.

Remember the distinction between enablement and running state: enabling wires a unit
into future boot or login targets, whereas starting activates it right now. A timer
can be active while its paired one-shot service sits idle, which is normal. The
[systemctl reference](https://www.freedesktop.org/software/systemd/man/latest/systemctl.html)
explains unit states, enablement, and masking.

## Configuration

Edit
[conf/systemd-units.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/systemd-units.toml).

The illustrative declarations below assume you provide `backup.timer` and
`old-backup.timer` as user units and the system provides `fstrim.timer`. Replace them
with your real units; these entries never create unit files:

```toml title="conf/systemd-units.toml (example)"
[linux]
units = [
  "backup.timer",
  { name = "fstrim.timer", scope = "system" },
  { name = "old-backup.timer", scope = "user", enabled = false },
]
```

A plain string declares an enabled user unit. A table requires `name` and `scope`
(`user` or `system`), with `enabled` defaulting to `true`. Include a supported suffix
(`.service`, `.timer`, `.socket`, `.target`, `.path`, `.mount`, `.automount`, or
`.swap`) and use the canonical unit name; if the live manager reports an alias,
Dotfiles asks you to declare the backing unit.

`graphical = true` is an optional setting for user units (default `false`). Outside an
active `graphical-session.target`, Dotfiles only enables graphical units and skips
checking or repairing their running state or Thunar preferences, letting their install
links start them when the graphical session begins. Inside an active graphical session,
normal runtime convergence applies. A failed session-state query stops the task rather
than guessing whether it is safe to start units.

With a live manager:

- `enabled = true` requires persistent enablement and, when the graphical policy allows,
  a running unit. Runtime-only enablement is not enough, and static units without
  install instructions cannot satisfy this declaration. Dotfiles retries a start that a
  condition previously skipped so systemd re-evaluates it; retrying a system unit can
  still prompt for `sudo` even if the condition keeps it stopped.
- `enabled = false` disables and stops the unit, including stopping a static unit that
  has no enablement links.
- If systemd reports an indirect unit, declare the unit that owns the install links;
  disabling an indirect unit can remove links belonging to another unit.
- Disabling is not masking, so another unit may still start it later. If an
  administrator enabled a user unit globally for all users, a per-user disable cannot
  prevent it from starting at next login; coordinate with the administrator first.
  A temporary mask also hides whether a unit will start once the mask expires, so
  Dotfiles cannot confirm permanent disablement until the mask expires or is lifted.
- When a locally installed user unit already has enablement links, Dotfiles reconciles
  changed `WantedBy` and `RequiredBy` targets using the same directive support as
  offline enablement; other units use native `systemctl` enablement.
- Dotfiles reloads the system manager when it reports a stale definition for a
  configured system unit.

User unit files typically arrive via home symlinks, while packages provide others.
System-scope changes can require `sudo`. Inside WSL, systemd must already be running
or degraded; editing `wsl.conf` does not start it in the current session.

Two managed user services carry extra runtime checks:

- **First-login service (`dotfiles-first-login.service`):** Once the deferred VS Code
  extensions marker is gone, an enabled unit can stay idle. When the marker exists (as
  a regular file, not a symlink), Dotfiles queues the idle service; it waits for the
  current update to release its run locks, installs the deferred extensions, and then
  takes the queue-directory lock on `~/.local/state/dotfiles/` (the same `flock` the
  producer holds while writing the marker) to compare and remove the marker. Newer
  selections remain queued and interrupted runs can retry from the marker. If the queue
  is busy, the service leaves it alone and retries after five minutes; if an update
  reports a busy queue, rerun once the other operation completes.
- **Thunar settings service:** After the managed service has run, Dotfiles checks its
  managed preferences and reruns the service if any value differs. Both the check and
  the service run the installed helper, including any overlay replacement.

Enabled units can perform consequential actions such as scheduled cleanup, so inspect
their commands and drop-ins before enabling an unfamiliar unit. On filesystems that
update directory access times, the daily `~/tmp` scan can prevent empty directories
from reaching the eight-day cleanup threshold, though scanning directory entries does
not refresh access times on regular files inside them. To stop managing a service and
also stop it running, apply an explicit `enabled = false` declaration first; removing
the entry alone does not disable it.

Without a live user manager, Dotfiles prepares per-user enablement links for the next
login instead of starting services. If manager access is denied or its state cannot be
determined, the task stops rather than editing links offline. Offline enablement
follows these rules:

- Masked units cannot be enabled offline, even when a drop-in supplies install targets.
- Disabling a unit removes matching enablement links even if the unit file is gone, and
  enablement links keep working after managed home links are materialized as local
  copies during removal.
- Unit-specific drop-ins for `WantedBy` and `RequiredBy` are honored: when targets
  change, Dotfiles adds the requested links and removes obsolete ones.
- Templates, other install relationships, and install directives inside shared drop-ins
  require a live user manager and are reported instead of partially enabled.
- Conflicting declarations for the same scope and unit are rejected.

See [What it does](#what-it-does) for when the task applies. Contributors modifying
offline unit lookup should start with the [offline enablement implementation](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/systemd_unit/offline.rs),
and changes to `[Install]` parsing live in the [target parser](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/systemd_unit/install_targets.rs)
and its [regression tests](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/resources/tests/systemd_unit_install_targets.rs).

To inspect both enablement and running state without changing anything (substituting
an actual user unit on your machine for `backup.timer`):

```sh
systemctl --user is-enabled backup.timer
systemctl --user status backup.timer
```

For system-scope units, omit `--user` and pass the system unit name. A disabled or
inactive unit often exits non-zero; compare the output with your declaration rather
than treating every non-zero exit as a broken install, and check service logs when a
unit fails to start instead of just re-enabling it.

## Preview and apply

Preview the desired enablement and running state for your selected units:

```sh
dotfiles update --only systemd --dry-run
```

Verify unit names, user/system scope, and any `enabled = false` entries, and resolve
missing unit files or manager requirements before [applying the
change](task-reference.md#preview-and-apply). Review unit commands first, especially
timers that run cleanup tasks. Afterward, confirm state with the `systemctl` commands
above (and check offline user enablement again after a real login).

## Removal

To stop a managed unit, set `enabled = false` and apply the reviewed change while the
declaration is still active. Deleting the entry alone does not disable or stop the
unit, and `dotfiles remove` never changes service state.
