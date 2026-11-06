---
title: User registry settings
description: Declared Windows current-user registry values.
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

Applies current-user values, preserving undeclared ones. This is separate from
machine-level Developer Mode and does not request brokered elevation. Some applications
read these settings only at startup. Remove does not restore former values.

## Configuration

Edit
[conf/registry.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/registry.toml).
Registry sections are labels, **not categories**. Every record applies on Windows
regardless of profile.

The following illustrative key demonstrates a string and a DWORD value.
Replace the path and value names with settings documented by the application;
creating this example key alone does not configure a real application:

```toml title="conf/registry.toml (example)"
[example-app]
path = 'HKCU:\Software\ExampleApp'

[example-app.values]
Theme = "dark"
ShowStatusBar = 1
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

Check the application's expected value type as well as its name: a numeric
string is not interchangeable with a DWORD. Dotfiles writes the declaration
you provide; it cannot establish that an undocumented application setting
still has the same meaning on a newer Windows version. Keep a record of the
original value before deliberately changing a preference you may want to undo.

## Preview and apply

Preview the current-user value changes:

```sh
dotfiles update --only registry --dry-run
```

Check each key, value name, and desired data against the application's
documentation, including the TOML type that selects DWORD or string storage.
Record values you may need to restore before [applying the
change](task-reference.md#preview-and-apply). If an application ignores a
stored value, check its expected type and restart requirements rather than
broadening the registry scope.

## Removal

Registry values remain after their declarations are removed or `dotfiles remove` runs.
Dotfiles does not retain their previous values. Restore the settings deliberately using
your backups or Windows configuration tools.
