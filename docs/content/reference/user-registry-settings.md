---
title: User registry settings
description: Manage Windows HKEY_CURRENT_USER registry preferences, choose DWORD or string types, preserve undeclared values, and plan manual restoration.
---

# User registry settings

The [Windows registry](https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry)
stores typed settings in named keys and values. This task manages only
HKEY_CURRENT_USER (HKCU), the current user's hive. It is intended for declared
user preferences, not arbitrary registry imports or machine policy.

| Task detail | Value |
|---|---|
| Selector | `registry` |
| Commands | `update` |
| Availability | Windows with configured values |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [system/registry.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/system/registry.rs) |

## What it does

Applies declared current-user registry values while leaving undeclared values untouched.
This task is separate from machine-level Developer Mode and never requests brokered
elevation. Some applications only read these settings when they start, and `dotfiles
remove` does not restore previous values.

## Configuration

Edit
[conf/registry.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/registry.toml).
Top-level section names here are labels, **not categories**, so every record applies
on Windows regardless of profile.

The illustrative entry below shows a string value alongside a DWORD value. Replace the
path and names with settings documented by your target application; adding this example
key does not configure a real app:

```toml title="conf/registry.toml (example)"
[example-app]
path = 'HKCU:\Software\ExampleApp'

[example-app.values]
Theme = "dark"
ShowStatusBar = 1
```

Each record requires `path` and `values`, and only `HKCU:\` paths are supported. Use
TOML literal single-quoted strings as shown so you do not have to escape backslashes.

| TOML value | Registry representation |
|---|---|
| Integer or boolean | `REG_DWORD` |
| Parseable hexadecimal string, such as `"0x0E"` | `REG_DWORD` |
| Ordinary string, including `"14"` | `REG_SZ` |

DWORD entries accept both unsigned and signed 32-bit integers: negative values keep
their two's-complement bit pattern, so `-1` writes `0xFFFFFFFF`, while hexadecimal
strings must fit in unsigned 32-bit range. Other TOML types serialize as strings.
Note that DWORD `14` and `"0x0E"` represent the same value, whereas DWORD `14` and
string `"14"` do not.

Configuration diagnostics check registry paths and DWORD ranges on every platform, and
Windows previews and updates reject invalid paths or values before processing any
entries. If an existing value cannot be read, Dotfiles leaves it untouched and reports
a failure while still applying other readable settings; once access is restored, retry
the update to recheck current values.

Verify both the value name and its expected type before editing, because a numeric
string is not interchangeable with a DWORD. Dotfiles writes whatever you declare and
cannot verify whether an undocumented setting still means the same thing on a newer
Windows build. Note the original value before changing any preference you might later
want to revert.

## Preview and apply

Preview the current-user value changes:

```sh
dotfiles update --only registry --dry-run
```

Check each key, value name, and data type against the application's documentation, and
record any original values you may want to restore before [applying the
change](task-reference.md#preview-and-apply). If an application ignores a written
value, check its expected type and restart behavior instead of expanding the registry
scope.

## Removal

Registry values stay in place after their declarations are removed or `dotfiles remove`
runs, because Dotfiles does not record prior values. Restore settings manually from
your notes or Windows configuration tools.
