---
name: system-files
description: >
  Use for conf/system-files.toml, tracked fragments under system/, or the task
  and resource that merge administrator-owned files below /etc. Not for user
  symlinks, systemd unit enablement, or Windows registry values.
---

# System Files

Implementation lives in
[config/system_files.rs](../../../cli/src/domains/system/config/system_files.rs),
[resources/system_file.rs](../../../cli/src/domains/system/resources/system_file.rs),
and [system_file/install.rs](../../../cli/src/domains/system/resources/system_file/install.rs).

## Non-obvious invariants

- Sources are relative to the declaring repo's `system/` directory; targets must
  be absolute paths below `/etc`. Entries do not probe package state.
- Merge strategies (`toml`, `ini`, `pam`, `create-only`) all preserve unrelated
  target content. Refuse malformed content, non-regular targets, differing
  `create-only` targets, missing PAM service files, and PAM fragments whose
  facilities do not exist.
- Privileged installation (`ATOMIC_INSTALL` + `system-file-helper`) must
  preserve symlink-component traversal checks, directory ownership/mode checks,
  same-filesystem private staging, `stat`/SHA-256 race verification before
  atomic `mv -fT` (and `ln -T` for new files so a concurrently created target
  cannot be clobbered), existing owner/group/mode/xattr preservation, and
  `root:root` `0644` defaults for new files.
