# Profiles and categories

A **profile** chooses a machine role. **Categories** combine that role with the
detected environment to select configuration sections. **Task filters** choose
which operations run against that configuration. These are separate controls:
`--profile base --only symlinks` means “link the base role's active sources,”
not “install every base task.”

## Built-in role profiles

| Profile | Role categories | Choose it for |
|---|---|---|
| `base` | `base` | A shell-focused machine, server, or minimal WSL environment |
| `desktop` | `base`, `desktop` | A workstation with GUI tools and desktop configuration |

`desktop` adds to `base`; it does not replace it. `base` is not a read-only or
user-files-only mode: active platform configuration and tasks can still install
packages, change the login shell, or manage system files. Review a dry run
before applying either profile.

Profile names are exactly `base` and `desktop`, in lowercase. `linux`,
`windows`, `arch`, and `wsl` are categories, not valid values for `--profile`.

## Automatic categories

| Category | How it becomes active |
|---|---|
| `linux` | The CLI runs on Linux |
| `windows` | The CLI runs on Windows |
| `arch` | Linux detection finds `/etc/arch-release` |
| `wsl` | Linux kernel release detection identifies WSL |

These categories are independent where appropriate: Arch inside WSL activates
both `arch` and `wsl`. The Windows executable and the Linux executable inside
WSL resolve different environments, even on the same physical machine.

| Environment | `base` profile categories | Additional category for `desktop` |
|---|---|---|
| Windows | `base`, `windows` | `desktop` |
| Non-Arch Linux | `base`, `linux` | `desktop` |
| Arch Linux | `base`, `linux`, `arch` | `desktop` |
| Non-Arch WSL | `base`, `linux`, `wsl` | `desktop` |
| Arch WSL | `base`, `linux`, `arch`, `wsl` | `desktop` |

Selecting `desktop` does not make unsupported platform tasks applicable.
For example, it does not add an apt or dnf package provider on non-Arch Linux;
the Linux package task looks for pacman.
Task discovery lists possible tasks; runtime capability checks decide which
can actually run.

## Section matching

For category-based configuration, hyphens mean **AND**: every tag must be
active. Matching sections accumulate.

```toml
[base]
packages = ["git"]

[arch-desktop]
packages = ["quickshell"]
```

On an Arch desktop both sections match. On an Arch base machine only `[base]`
matches. `[arch-desktop]` and `[desktop-arch]` express the same condition;
neither means “Arch OR desktop.” Because `base` is always active, `[base]`
applies to both roles.

Use the known categories rather than inventing machine names as section tags:
unknown tags are configuration errors, not a way to declare a new profile.
Not every TOML table is a category section; registry tables, for example,
represent named registry entries. See [Category sections](CONFIGURATION.md#category-sections)
for file-specific rules.

## Resolution priority

Configuration-aware commands resolve the role in this order:

1. `--profile <name>`.
2. A nonempty `DOTFILES_PROFILE`.
3. Repository-local Git config `dotfiles.profile`.
4. An interactive profile prompt, when the command permits it.

The first selected value is validated. An invalid environment or persisted
value does not fall through to a lower-priority choice. Whitespace is not
trimmed from profile values: `DOTFILES_PROFILE=" base "` is invalid.

An explicit profile affects only that invocation; it does **not** rewrite the
saved choice. The interactive prompt does persist its answer. This can happen
during `check` or `--dry-run`, so use an explicit profile when you do not want
the prompt or its persistence.

From the intended checkout, inspect the saved choice:

```bash
git config --local --get dotfiles.profile
```

No output with a nonzero Git status normally means the key is unset.
To deliberately change the default for future runs:

```bash
git config --local dotfiles.profile base
```

This changes local repository settings, not tracked configuration. Linked
worktrees can share this local Git configuration; use `--profile` for an
unambiguous per-run selection.

`tasks` never opens the profile prompt or persists a selection. It requires a
profile from the first three sources. Engine commands also require one of
those sources when stdin is not a terminal, `--non-interactive` is passed, or
`CI` is present. `log` and help/version output do not require a profile.

## Changing profiles or categories

### Preview a different role

With an existing CLI on PATH, run from the repository root:

```bash
dotfiles tasks --root . --profile desktop
dotfiles install --root . --profile desktop --no-repo-update --dry-run --verbose
```

The second command previews task changes, but still has the bookkeeping and
external-probe effects described in [Dry-run scope](USAGE.md#dry-run-scope).
It does not update the saved profile.

**Switching roles is not a migration or cleanup operation.** Selecting `base`
after installing `desktop` stops selecting desktop-only entries; it does not
uninstall their packages, disable their services, or remove previously
installed links. Plan any cleanup while the original profile and sources are
still available. `uninstall` also uses the current selection and only removes
the integrations in [Uninstall tasks](TASKS.md#uninstall-tasks).

If configuration seems missing, distinguish:

- **Section not selected:** one of its category tags is inactive.
- **Task filtered out:** `--only`/`--skip` excluded the operation.
- **Task not applicable or unmet:** the host/tool capability cannot perform it.
- **Already current:** the default output omits that task; use `--verbose`.

### Extend the built-in model

Profiles are implemented in `cli/src/app/config/profiles.rs` and
`cli/src/app/config/profiles/`; category parsing/matching lives in
`cli/src/infra/config/category_matcher.rs`. Adding a role or environment
category is a code change, not just a new TOML heading. Keep resolution,
known-category preflight, applicable configuration, and tests aligned.
Use [Testing](TESTING.md) for the validation workflow.
