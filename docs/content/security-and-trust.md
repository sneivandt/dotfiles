---
title: Security and trust
description: Understand trust boundaries, verified downloads, elevation, private overlays, and the privacy of logs and secrets.
---

# Security and trust

Dotfiles can change your account's configuration and request privileged changes.
Its checks help prevent mistakes and reject invalid inputs, but they do not make
an untrusted checkout, package, or overlay safe to run.

The practical rule is simple: **review what you will execute and what you will
share**. Dry-run, checksums, and a green CI result each answer a narrower
question than "is this safe?"

## Trust boundaries

| What you are trusting | Why it matters |
|---|---|
| The checkout and future repository updates | Shell/editor configuration, hooks, and managed scripts can execute code; linked applications may reload edits immediately |
| Configuration and profiles | Valid values may still select work or paths you did not intend |
| A private overlay | Its content and scripts run with your access, not in an isolated plugin container |
| Release binaries and upstream packages | Verification ties bytes to a publisher/build; it is not a review of all dependencies |
| Tools found on `PATH` | Package managers, installers, and agent tools execute outside the CLI's control |
| An elevated task | Scoped elevation reduces privilege exposure, but does not sandbox the code it runs |
| Logs and diagnostic output | Paths, arguments, and tool output can contain private information |

Back up files you care about before applying configuration. Task selection is
not a trust policy, and removal is not rollback.

## Release downloads

The [Linux](https://github.com/sneivandt/dotfiles/blob/main/dotfiles.sh) and [Windows](https://github.com/sneivandt/dotfiles/blob/main/dotfiles.ps1) wrappers download the
platform asset and its SHA-256 checksum file over HTTPS. They verify the matching
asset entry before accepting the download. The CLI's
[self-update path](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/self_update.rs) also verifies
downloads before installing them.

A checksum detects corruption or a mismatch with the published checksum. It
does not protect against a compromised publisher who can replace both. Build
attestations add evidence about the repository and workflow that produced the
asset; that repository, workflow, dependencies, and account still need to be
trusted.

Verification applies to downloads, not continuous integrity monitoring of every
existing executable. `--build` compiles the local checkout and dependencies
instead; it does not verify a release attestation.

### Build provenance verification

Provenance verification uses `gh attestation verify` with the release repository
constraint. A wrapper resolves its release repository from configuration/origin;
the Rust binary uses the repository embedded at build time. For this repository's
Linux release, manual verification is:

```sh
gh attestation verify bin/dotfiles --repo sneivandt/dotfiles
```

Use `bin/dotfiles.exe` for Windows. For a fork's release, use that fork's
repository rather than the upstream name.

| Condition | Wrapper bootstrap | CLI self-update |
|---|---|---|
| `gh` is present and verification succeeds | Accept after checksum verification | Accept after checksum verification |
| `gh` is absent | Warn; accept with checksum verification only | Reject the update |
| Verification or authentication fails | Reject the download | Reject the update and preserve the installed binary |
| `DOTFILES_SKIP_ATTESTATION=1` | Skip provenance, but still check the checksum | Same |
| CLI `--skip-attestation` | Forwarded to Rust; does not bypass wrapper verification | Skip provenance for this invocation |

The missing-`gh` bootstrap exception allows first use before the tool is
installed. It is weaker verification, not a successful attestation check.
Do not enable a bypass just to silence an unexplained failure.

Automatic GitHub CLI calls disable prompts and Linux desktop-keyring access to
avoid hidden authentication dialogs, including under WSL. If credentials exist
only in that keyring, supply `GH_TOKEN` or `GITHUB_TOKEN` in the invoking
environment. This affects the child process, not your system keyring or normal
interactive `gh`. Missing credentials never authorize an unverified update.
See [Troubleshooting](troubleshooting.md) for recovery.

## Elevation

Start the CLI as your normal user. It arranges privileges for selected work
rather than requiring the whole session to run as root or Administrator.

On **Linux**, it uses cached sudo credentials or, in an interactive session,
primes them in the foreground. On **Windows**, tasks needing elevation run in
one short-lived UAC-elevated child restricted to their selectors.
Non-interactive/CI sessions do not prompt for fresh sudo credentials or UAC
consent.

If privileges are unavailable or declined, the affected work is unmet and
failure-blocking dependents cannot proceed. Unrelated tasks can continue.
`--fail-on-skip` and CI's strict completion policy make unmet work fail the run.
A skipped privileged task did not apply the change.

This is not privilege removal: if you launch the CLI from an elevated shell,
it keeps those privileges. System packages, system-scoped units, and files under
`/etc` are examples of privileged work. Windows symlinks have additional
[platform requirements](windows-and-wsl.md).

## Private overlays

An overlay extends the public configuration; it is not a security boundary.
Confirm which overlay is active before applying changes or sharing output.
Review both its declarations and the content deployed into shells, editors, and
agent tools.

Only an overlay's `conf/scripts.toml` defines script tasks; the public
configuration does not. Their `--check` and `--dryrun` modes are cooperative
contracts. A script can ignore them, access the network, print private values,
or change files. **Dry-run is not a sandbox.**

Path validation constrains specific managed resources, such as relative home
links and system-file destinations under `/etc`. It does not confine every
external command to those directories. See
[Private overlays](private-overlays.md) for setup, merging, and the script protocol.

## Secrets

Keep credentials, private keys, tokens, connection strings, and private
machine/overlay values out of tracked files, examples, fixtures, and uploads.

The [pre-commit scanner](git-hooks.md#scan-scope) checks added staged text using
versioned patterns and redacts matches. It is local and bypassable; it does not
scan every historical, binary, or encoded value. A clean scan is useful, but not
proof that a patch contains no secrets.

### Logs are a separate disclosure boundary

Treat logs as potentially sensitive even when the console message looks
harmless. By default, command diagnostics retain failed stdout/stderr and
successful stderr. Successful stdout normally contributes a byte count, but
callers can request full output. Domain messages and overlay output can contain
private data too.

There is no universal secret scrubber. Contributors changing logged output
should follow the [logging and redaction contracts](architecture.md#error-handling-and-observability).

New Unix log directories use owner-only `0700` permissions; new logs and lock
sidecars use `0600`, further restricted by the umask. Existing permissions are
not repaired. Windows logs inherit filesystem ACLs.

Inspect logs locally and share only the smallest sanitized excerpt. If a real
secret reaches a commit or shared log, **revoke or rotate it first**. Deleting
the latest copy does not remove history, downloaded artifacts, or other copies.
Coordinate any history cleanup separately.

## Dependency and CI controls

[CI](ci-and-releases.md) runs selected build, test, and dependency checks. Its aggregate gate
requires selected jobs to succeed; coverage and mutation reports are
informational. The separate scheduled dependency workflow runs weekly and on
manual dispatch. Enable failure notifications and GitHub's Dependabot features
separately rather than assuming a workflow does that for you.

[Release publication](ci-and-releases.md#release-selection) is tied to successful
same-repository push CI on `main` and the exact tested commit. Publishing uses
scoped permissions, checksums, and attestations; attestations must be retrievable
before the release is published. These controls do not protect against every
compromised maintainer, dependency, or approved workflow.

## Safe contribution practices

Review new executable content, external sources, privilege requests, and logged
data explicitly. Keep workflow permissions narrow and external Actions pinned
to full commit SHAs. Preserve failure reporting, idempotency, and preview tests
without calling those controls isolation or rollback.

Use synthetic secrets and isolated fixtures, never private overlay excerpts.
[Contributing](contributing.md) and [Testing](testing.md) describe the workflow.

## Reporting a vulnerability

Do not put credentials, private configuration, or sensitive exploit details in
a public issue. Check the repository's
[Security page](https://github.com/sneivandt/dotfiles/security) for an available
private reporting channel before sending details. If none is advertised, ask
how to contact the maintainer without including sensitive material.

This guide does not assert that private reporting is enabled or promise a
disclosure response process.
