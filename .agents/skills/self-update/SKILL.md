---
name: self-update
description: >
  Use for Rust binary self-update under cli/src/domains/dotfiles/self_update*
  (release selection/cache, download verification, binary replacement, smoke
  tests, restart integration) and for dotfiles.sh / dotfiles.ps1 wrapper
  bootstrap, verification, --build handling, and argument forwarding. Not for
  Git repository synchronization, package upgrades, managed shell startup
  (use shell-configuration), or Git hooks (use git-hooks-patterns).
---

# Self Update and Wrapper Bootstrap

Wrappers (`dotfiles.sh`, `dotfiles.ps1`) own bootstrap before a binary exists;
Rust (`cli/src/domains/dotfiles/self_update.rs` and `self_update/`) owns normal
binary self-update. Git checkout sync is separate (`cli/src/domains/repository/`).

## Rust binary self-update

- Preserve dev-build/installed-path applicability, explicit skip policy, release
  ordering (do not treat arbitrary tags as newer), and offline-discovery vs
  failed-install distinction.
- Trust and recovery sequence (`self_update/install.rs`):
  1. Verify SHA-256 checksum and required provenance on the staged download
     before touching the working binary—never silently weaken attestation policy.
  2. Smoke-test **both** the staged binary before swap and the installed binary
     after swap against the exact expected release string (`dotfiles <tag>`),
     not merely exit code 0, so a runnable wrong release cannot become eligible
     for launcher recovery.
  3. Install from the same verified staged bytes, keep the old binary for
     rollback, and preserve both the original failure and any rollback failure.
- Preserve guarded restart and forwarded arguments so execution never loops or
  continues with stale process-generation inputs.

## Wrapper bootstrap

- Wrappers own only production download/bootstrap, checksum/provenance
  verification, `--build` source mode, pre-binary flags, and verbatim
  forwarding of all other arguments—domain behavior belongs in Rust.
- Keep `dotfiles.sh` (POSIX `#!/bin/sh`, no Bashisms) and `dotfiles.ps1`
  (explicit native exit-code checks—`$ErrorActionPreference = 'Stop'` does not
  catch every non-zero native exit) in parity.
- Never reconstruct forwarded arguments via `eval` or string interpolation;
  verify downloads before execution or cache replacement and preserve the
  working binary on failure. Cover consumed `--build`, empty arguments, spaces,
  arguments after `--`, stdout/stderr routing, and child exit code.
- Wrapper progress uses the CLI's compact ` · `-separated style; keep warnings
  and errors on stderr with their existing prefixes.

Use injected HTTP/provenance fixtures and wrapper tests from
[Testing](../../../docs/content/development/testing.md#choosing-coverage); never
replace the live `bin/dotfiles` to test a patch. Use `ci-cd-patterns` when
changing release asset/tag/provenance production.
