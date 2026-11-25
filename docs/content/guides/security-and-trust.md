---
title: Security and trust
description: Understand trust boundaries, verified downloads, elevation, private overlays, and the privacy of logs and secrets.
---

# Security and trust

Decide whether you trust the checkout and any private overlay **before
running their code**, including a wrapper or a preview. Dotfiles can change
your home and request system privileges. A verified CLI can still apply
configuration you didn't intend.

## Trust boundaries

| Action | What to review first |
|---|---|
| Run a wrapper | Check the wrapper and its release publisher. Verification authenticates the download, not every dependency. |
| Preview an overlay | Read its script declarations and the scripts they invoke. Even the probes run with your account access. |
| Apply an update | Confirm the profile, overlay, destinations, package sources, and tools on `PATH`. Back up important files; `remove` is not a rollback. |
| Sync a checkout | Shell settings, editor configuration, and hooks can become live as soon as their sources change. To stay on reviewed sources, use `--no-repo-update` for both preview and apply. |
| Approve elevation | Check which task needs privileges. Task selection limits what is scheduled, not what an external installer can do. |
| Enable a service or timer | Check what it will do after dotfiles exits, including [automatic file cleanup](https://github.com/sneivandt/dotfiles/blob/main/symlinks/config/systemd/user/clean-home-tmp.service). |
| Share diagnostics | Read the excerpt for private paths, arguments, or tool output before posting it. |

## Release downloads

Both the [Linux](https://github.com/sneivandt/dotfiles/blob/main/dotfiles.sh)
and [Windows](https://github.com/sneivandt/dotfiles/blob/main/dotfiles.ps1)
wrappers download the matching release binary and its SHA-256 checksum file
over HTTPS, rejecting any mismatch before accepting the file. The CLI's
[self-update path](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/dotfiles/self_update.rs)
verifies downloads the same way before swapping binaries.

A matching checksum says the bytes match the publisher's checksum file. It
doesn't independently identify the publisher. Build provenance is a separate
check: the attestation identifies the build, and the verifier requires it to
come from the expected repository. Neither check reviews the code for you.

Verification runs when downloading a release rather than continuously
monitoring binaries already on disk. Passing `--build` compiles the local
checkout and its dependencies instead of checking a release attestation.

### Build provenance verification

Wrappers use the GitHub repository named by `origin`, unless you set
`DOTFILES_RELEASE_REPO`. Self-update uses the publisher embedded in the binary;
changing `origin` doesn't change it. To verify this repository's Linux release
binary manually:

```sh
gh attestation verify bin/dotfiles --repo sneivandt/dotfiles
```

Use `bin/dotfiles.exe` on Windows. If the release came from a fork, substitute
that fork's name only if it is the publisher you intended to trust.

Both bootstrap and self-update require GitHub CLI (`gh`) and enforce checksum
and provenance verification. If `gh` is missing, attestation fails, or
authentication fails, the download is rejected and any existing binary stays
untouched. Install GitHub CLI before bootstrapping, or use `--build` to compile
from a checkout you've reviewed.

Setting `DOTFILES_SKIP_ATTESTATION=1` skips provenance checks in both paths;
the CLI's `--skip-attestation` flag applies only to self-update, not bootstrap.
Neither bypasses SHA-256 checksum verification. Avoid using either bypass to
work around an unexplained failure.

Automatic `gh` calls disable interactive prompts and Linux desktop keyring
access, including under WSL. A working interactive login may therefore fail
during verification. If your credentials live only in the keyring, supply
`GH_TOKEN` or `GITHUB_TOKEN` in the invoking environment, not in tracked
configuration or a shared diagnostic. See
[Wrapper download fails](troubleshooting.md#wrapper-download-fails).

## Elevation

Launch dotfiles from your normal user account. Starting it from an elevated
shell gives the whole run those privileges; dotfiles doesn't drop them.

On Linux, dotfiles reuses cached `sudo` credentials or primes them in the
foreground when running interactively. On Windows, machine-scoped Developer
Mode work runs in a single short-lived UAC-elevated child while home symlinks
and user registry settings remain in your original process (even if UAC
authenticates a different Administrator account). Dotfiles itself doesn't
prompt for `sudo` or UAC in a non-interactive or CI run.

If you decline, tasks requiring that authorization don't run. Their blocking
dependents can't proceed, but independent tasks can. `--fail-on-skip` (also
enabled by CI) makes unavailable privileges a failure. On Windows, a parent
row saying `ran in elevated session` means the child **did** run the task;
inspect its linked log. See [Windows and WSL](windows-and-wsl.md) for symlink
requirements.

## Private overlays

A private overlay layers personal configuration onto the public repo; it does
not isolate or sandbox that code. Verify which overlay is active before
applying changes or sharing output, and review both its TOML declarations and
any files it deploys into shells, editors, or AI agents.

Only an overlay's `conf/scripts.toml` can define script tasks. Read the script
it names before an update **or removal** preview: the CLI executes the script
to check state. `--check`, `--check-remove`, and `--dryrun` are requests, not
restrictions. A script can still write files, call the network, or log secrets.

Path validation restricts managed resources like home-relative symlinks and
`/etc` destinations, but it cannot confine arbitrary external commands. See
[Private overlays](private-overlays.md) for setup, merging, and script
requirements.

## Secrets

Never place credentials, private keys, tokens, connection strings, or private
machine/overlay details in tracked files, examples, test fixtures, or public
uploads.

The [pre-commit scanner](../development/testing.md#sensitive-pattern-scanning)
checks newly staged lines against known patterns. A match blocks the commit
and is redacted **in the report**; you still need to remove it from the staged
content. A passing scan does not clear the history, binaries, or unrecognized
secrets.

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
