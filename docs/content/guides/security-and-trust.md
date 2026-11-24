---
title: Security and trust
description: Understand trust boundaries, verified downloads, elevation, private overlays, and the privacy of logs and secrets.
---

# Security and trust

Dotfiles modifies your account configuration and can request privileged system
changes. Its validation and verification checks catch mistakes and reject
invalid inputs, but they cannot make an untrusted checkout, package, or overlay
safe to run.

Always review what you are about to apply or share. Previews show planned work,
checksums verify downloaded bytes, and CI covers tested code paths, yet none of
them replaces judging whether a given configuration belongs on your machine.

## Trust boundaries

| What you are trusting | Why it matters |
|---|---|
| The checkout and future repository updates | Shell/editor configuration, hooks, and managed scripts run code, and linked apps can reload edits immediately |
| Configuration and profiles | Syntactically valid entries can still select paths or tasks you did not intend |
| A private overlay | Its files and scripts run with your full account access rather than in an isolated sandbox |
| Release binaries and upstream packages | Verification ties bytes to a publisher and build, not a review of every dependency |
| Tools found on `PATH` | Package managers, installers, and agent tools execute outside the CLI's control |
| Elevated tasks | Scoped elevation reduces privilege exposure, but it does not sandbox the code it runs |
| Enabled services and timers | They continue running after dotfiles exits, including [automatic file cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service) |
| Logs and diagnostic output | Paths, arguments, and tool output can capture private details |

Back up important files before applying configuration. Task filtering isn't a
security policy, and `remove` isn't a full rollback.

## Release downloads

Both the [Linux](https://github.com/sneivandt/dotfiles/blob/main/dotfiles.sh)
and [Windows](https://github.com/sneivandt/dotfiles/blob/main/dotfiles.ps1)
wrappers download the matching release binary and its SHA-256 checksum file
over HTTPS, rejecting any mismatch before accepting the file. The CLI's
[self-update path](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/self_update.rs)
verifies downloads the same way before swapping binaries.

A checksum confirms bytes weren't corrupted or altered in transit, though it
cannot stop a compromised publisher who controls both files. Build attestations
tie the asset back to a specific repository and workflow, which you still need
to trust along with its dependencies and maintainers.

Verification runs when downloading a release rather than continuously
monitoring binaries already on disk. Passing `--build` compiles the local
checkout and its dependencies instead of checking a release attestation.

### Build provenance verification

Provenance verification runs `gh attestation verify` scoped to the release
repository. Wrappers derive that repository from configuration or `origin`,
while the Rust binary uses the repository embedded at compile time. To verify
this repository's Linux release binary manually:

```sh
gh attestation verify bin/dotfiles --repo sneivandt/dotfiles
```

Use `bin/dotfiles.exe` on Windows, and substitute your fork's repository name
when verifying a fork's release.

Both bootstrap and self-update require GitHub CLI (`gh`) and enforce checksum
and provenance verification. If `gh` is missing, attestation fails, or
authentication fails, the download is rejected and any existing binary stays
untouched. Install GitHub CLI before bootstrapping, or use `--build` to compile
from a checkout you've reviewed.

Setting `DOTFILES_SKIP_ATTESTATION=1` skips provenance checks in both paths;
the CLI's `--skip-attestation` flag applies only to self-update, not bootstrap.
Neither bypasses SHA-256 checksum verification. Avoid using either bypass to
work around an unexplained failure.

During self-update, the CLI fetches the checksum before the binary and rejects
binary responses over 128 MiB. Network retries apply only to downloads, not to
local staging failures.

Automatic GitHub CLI calls disable interactive prompts and Linux desktop
keyring access so background checks never hang on hidden dialogs (including
under WSL). If your `gh` credentials live only in a desktop keyring, export
`GH_TOKEN` or `GITHUB_TOKEN` in your shell before running dotfiles. This
setting affects only the child `gh` process, not your system keyring or normal
interactive `gh` usage, and missing credentials never allow an unverified
update. See [Troubleshooting](troubleshooting.md) if verification fails.

## Elevation

Always launch dotfiles under your normal user account so it requests elevated
privileges only for the specific tasks that need them.

On Linux, dotfiles reuses cached `sudo` credentials or primes them in the
foreground when running interactively. On Windows, machine-scoped Developer
Mode work runs in a single short-lived UAC-elevated child while home symlinks
and user registry settings remain in your original process (even if UAC
authenticates a different Administrator account). Non-interactive and CI runs
never prompt for `sudo` or UAC.

When privileges are unavailable or declined, affected tasks are skipped along
with their blocking dependents, while independent tasks still run. Passing
`--fail-on-skip` (or running in CI) turns those skips into command failures.
A skipped privileged task makes no changes.

Dotfiles does not drop privileges if you launch it from an already-elevated
shell. System packages, system-scoped systemd units, and files under `/etc` are
typical privileged tasks; see [Windows and WSL](windows-and-wsl.md) for
Windows-specific symlink requirements.

## Private overlays

A private overlay layers personal configuration onto the public repo; it does
not isolate or sandbox that code. Verify which overlay is active before
applying changes or sharing output, and review both its TOML declarations and
any files it deploys into shells, editors, or AI agents.

Only an overlay's `conf/scripts.toml` can define script tasks (the main
repository never defines them). Remember that a script's `--check` and
`--dryrun` modes are cooperative conventions rather than a sandbox: a script
can ignore flags, call the network, log secrets, or write files. **Dry-run is
not a sandbox.**

Path validation restricts managed resources like home-relative symlinks and
`/etc` destinations, but it cannot confine arbitrary external commands. See
[Private overlays](private-overlays.md) for setup, merging, and script
requirements.

## Secrets

Never place credentials, private keys, tokens, connection strings, or private
machine/overlay details in tracked files, examples, test fixtures, or public
uploads.

The [pre-commit scanner](../development/testing.md#sensitive-pattern-scanning)
scans newly staged lines against versioned patterns and redacts matches. It is
a helpful local guardrail, not a guarantee: it can be bypassed and does not
inspect git history, binaries, or encoded payloads.

### Sharing logs

Treat every log file as potentially sensitive, even when the terminal summary
looks harmless. By default, command diagnostics keep stdout and stderr for
failed commands plus stderr for successful ones (and callers can opt in to full
stdout), and domain or overlay messages can include private paths and output.

Because there is no universal secret scrubber, contributors touching logged
output must follow the
[logging and redaction contracts](../development/architecture.md#error-handling-and-observability).

On Unix, new log directories are created with mode `0700`, and new log files
and lock sidecars with `0600` (further narrowed by your umask); permissions on
existing files are not retroactively changed. On Windows, logs inherit normal
filesystem ACLs.

Inspect logs locally and share only the minimal sanitized excerpt needed. If a
real secret ever reaches a commit or shared log, **revoke or rotate it
immediately**: removing the latest copy does not erase git history, artifacts,
or downstream copies. Coordinate any history cleanup afterward.

## Dependency and CI controls

A green [CI run](../development/ci-and-releases.md) confirms that the checks
selected for that commit passed. Coverage uploads are informational, and
[mutation testing](../development/ci-and-releases.md#scheduled-mutation-testing)
runs on its own schedule without blocking releases. The dependency workflow
runs weekly or on demand; configure Dependabot and alerts separately.

[Release publication](../development/ci-and-releases.md#release-selection)
requires passing same-repository push CI on `main` for the exact tested commit.
Releases use scoped permissions, checksums, and build attestations that must be
verifiable before publication completes. Even so, these controls don't defend
against every compromised maintainer, dependency, or workflow.

## Safe contribution practices

Review new executable content, external sources, privilege requests, and logged
fields carefully. Keep workflow permissions minimal and pin external GitHub
Actions to full commit SHAs. Preserve error reporting, idempotency, and preview
tests without misrepresenting them as sandboxing or rollback.

Always use synthetic secrets and isolated test fixtures rather than excerpts
from a private overlay. See [Contributing](../development/contributing.md) and
[Testing](../development/testing.md) for the full contributor workflow.

## Reporting a vulnerability

Never post credentials, private configuration, or sensitive exploit details in a
public issue. Check the repository's
[Security page](https://github.com/sneivandt/dotfiles/security) for a private
reporting channel first; if none is listed, ask how to reach the maintainer
privately without including sensitive details.

If you're diagnosing a verification, elevation, or runtime error, continue to
[Troubleshooting](troubleshooting.md).
