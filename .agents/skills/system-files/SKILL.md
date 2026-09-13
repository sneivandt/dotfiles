---
name: system-files
description: >
  Use for conf/system-files.toml, tracked fragments under system/, or the task
  and resource that merge administrator-owned files below /etc. Not for user
  symlinks, systemd unit enablement, or Windows registry values.
---

# System Files

## Ownership

| Concern | Owner |
|---|---|
| desired target, source, merge strategy, and selectors | `conf/system-files.toml` |
| public fragments | `system/` |
| parsing and path safety | `cli/src/domains/system/config/system_files.rs` |
| merge, state, metadata, and privileged install | `cli/src/domains/system/resources/system_file.rs` |
| task applicability and elevation planning | `cli/src/domains/system/system_files.rs` |
| install catalog membership | `cli/src/app/catalog.rs` |

## Contracts

- Use category sections for role and environment selection. `base` and custom
  categories come from profiles; `linux`, `windows`, `arch`, and `wsl` come
  from platform detection. Compound sections require every category.
- Sources are relative to the declaring repository's `system/` directory;
  targets must be absolute paths below `/etc`.
- `toml` recursively merges tables, `ini` converges fragment keys within their
  sections, and `pam` places module rules after the matching facility stack.
  All strategies preserve unrelated target content.
- Refuse malformed content, non-regular targets, missing PAM service files,
  and PAM fragments whose facilities do not exist.
- Install through the executor as `root:root` mode `0644` from a unique staged
  file. Constructors, state discovery, and dry runs stay read-only.
- Keep package ownership in `conf/packages.toml`; selection may align a fragment
  with a package profile, but system-file entries do not probe package state.

Cover selector filtering, unsafe and duplicate paths, every merge strategy,
missing/correct/incorrect/invalid state, staging arguments, dry-run safety, and
convergence. Use [Testing](../../../docs/TESTING.md#choosing-coverage) for the
validation sequence.
