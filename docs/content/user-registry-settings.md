---
title: User registry settings
description: Declared Windows current-user registry values.
---

# User registry settings

| Task detail | Value |
|---|---|
| Selector | `registry` |
| Commands | `update` |
| Availability | Windows with configured values |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [system/registry.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/registry.rs) |

## What it does

Applies current-user values, preserving undeclared ones. This is separate from
machine-level Developer Mode and does not request brokered elevation. Some applications
read these settings only at startup. Remove does not restore former values.

## Configuration

Edit
[conf/registry.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/registry.toml).
Registry sections are labels, **not categories**. Every record applies on Windows
regardless of profile:

```toml title="conf/registry.toml (excerpt)"
[explorer]
path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer'

[explorer.values]
EnableAutoTray = 0
```

Each record needs `path` and `values`; only `HKCU:\` is supported. Use TOML literal
strings, as above, to avoid escaping backslashes.

| TOML value | Registry representation |
|---|---|
| Integer or boolean | `REG_DWORD` |
| Parseable hexadecimal string, such as `"0x0E"` | `REG_DWORD` |
| Ordinary string, including `"14"` | `REG_SZ` |

Use DWORD values in the unsigned 32-bit range. Other TOML types are serialized as
strings; explicit strings are clearer. DWORD `14` and `"0x0E"` represent the same
desired value; DWORD `14` and string `"14"` do not.

Fields and value types are checked on non-Windows systems too, but the settings are only
applied on Windows. Existing undeclared values are preserved.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only registry --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Registry values remain after their declarations are removed or `dotfiles remove` runs.
Dotfiles does not retain their previous values. Restore the settings deliberately using
your backups or Windows configuration tools.
