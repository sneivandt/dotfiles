# Security model

Dotfiles runs with access to your account and can request privileged changes.
Its controls reduce accidental exposure and constrain specific operations;
they do not make an untrusted checkout, overlay or package safe to execute.
This guide documents those boundaries, not a vulnerability audit or a promised
private disclosure service.

## Trust boundaries

| Input/boundary | What to trust or review | What the CLI does not guarantee |
|---|---|---|
| Checkout and managed content | Rust, wrappers, hooks, shell/editor configuration and future repository updates | That applying configuration is harmless; linked applications may load changed code immediately |
| Configuration and profiles | Selected records, paths and the combined public/private desired state | That syntactically valid values implement your intended policy |
| Overlay | Local repository, fragments and executable scripts | Isolation from your home, network or other accessible state |
| Release binary | GitHub release metadata and provenance, plus the local copy after verification | That checksum/provenance proves absence of malicious or vulnerable code |
| External tools/packages | Package providers, AUR recipes, APM packages/plugins and executables resolved on PATH | Independent review of every upstream installer or transitive dependency |
| Elevation | The selected privileged tasks and all code they call | A sandbox around a task or protection from an already-compromised account |
| Logs and diagnostics | Paths, command arguments and captured output before sharing | Automatic removal of every secret or private value |

Review the selected work and its sources before install/update. Selection,
idempotency and dry-run are operational controls, not trust decisions.

## Release downloads

The [POSIX wrapper](../dotfiles.sh), [PowerShell wrapper](../dotfiles.ps1) and
[self-update path](../cli/src/domains/dotfiles/self_update.rs) select a release,
download its platform asset and SHA-256 metadata over HTTPS, and verify the
checksum before accepting the download. Checksum lookup uses the matching
asset entry from release metadata. Provenance policy differs between initial
bootstrap and self-update, as below.
The POSIX bootstrap stages each download privately beside the cached binary and
publishes it only after verification. Failed downloads or verification do not
delete a binary published by another concurrent bootstrap.

A checksum detects mismatched/corrupt bytes relative to the published checksum.
If the publisher or release metadata is compromised, an attacker can publish
matching bytes and checksums. GitHub build attestations add evidence linking
the asset to a repository/workflow build; they do not replace review of that
repository, its dependencies, the workflow or GitHub account access.

These controls run on downloads, not as continuous integrity monitoring of
every existing local executable. `--build` instead compiles the trusted local
checkout and dependencies; it does not verify a release attestation.

## Build provenance verification

Verification runs `gh attestation verify <asset> --repo sneivandt/dotfiles`.
The release workflow attests assets and verifies that attestations can be
retrieved **before publishing** the release. The client supplies the repository
constraint; it does not maintain a separate allowlist of approved commits.

| Situation | Wrapper bootstrap | CLI self-update |
|---|---|---|
| `gh` available and verification succeeds | Accept after checksum verification | Accept after checksum verification |
| `gh` absent | Warn and continue with checksum verification only | Reject the update download |
| `gh` present but verification/authentication fails | Reject bootstrap | Reject the update; preserve the installed binary |
| `DOTFILES_SKIP_ATTESTATION=1` | Skip provenance, still verify checksum | Skip provenance, still verify checksum |
| CLI `--skip-attestation` | Forwarded to Rust; does not bypass wrapper verification | Skip provenance for this invocation's self-update |

The bootstrap exception permits first use before `gh` is installed. It is a
weaker verification path, not successful provenance verification.
Self-update retries transient/verification failures up to three times; an
authentication-required result is reported immediately.
See [the implementation](../cli/src/domains/dotfiles/self_update/attestation.rs).

To verify an existing downloaded Linux binary manually:

```bash
gh attestation verify bin/dotfiles --repo sneivandt/dotfiles
```

Use `bin/dotfiles.exe` on Windows. Check the reported repository and provenance;
do not set the bypass merely to silence an unexplained error. For operational
diagnosis, see [Troubleshooting](TROUBLESHOOTING.md).

## Elevation

The [elevation broker](../cli/src/app/commands/execution/elevation.rs) assesses
selected tasks before dispatch. It does not require running the whole CLI as
root/administrator:

- **Linux:** cached sudo credentials can be used without a prompt; when
  necessary and interactive, credentials are primed in the foreground before
  privileged commands run. Non-interactive/CI runs do not prompt for fresh
  credentials.
- **Windows:** selected elevating tasks run in one short-lived UAC-elevated
  child restricted to their selectors. The parent remains unelevated and the
  child cannot request another elevated child. Non-interactive/CI sessions do
  not request UAC consent.
- **Unavailable or declined privilege:** affected tasks become unmet work and
  blocking dependents cannot proceed. Unrelated tasks can continue. Strict
  completion (`--fail-on-skip` or CI) turns unmet work into failure;
  “skipped” does not mean the privileged change happened.

The normal parent stays unprivileged **when launched that way**. Launching the
CLI from an already elevated shell gives it that shell's privileges; the engine
does not drop them.

Examples of scoped privileged work include system packages, system-scoped units
and merged files below `/etc`. Windows file symlinks may need elevation without
Developer Mode; directory junctions provide a non-elevated fallback. Current
registry configuration is user-scoped. These choices minimize prompting, not
the consequences of malicious code inside an approved task.

## Private overlays

An overlay extends the public configuration; it is not an untrusted plugin
container. It can be selected explicitly or through the repository's supported
environment/persisted selection. Confirm which overlay is active before sharing
output or applying changes.

The [configuration loader](../cli/src/app/config/mod.rs) appends supported main
then overlay records, retaining source origins. Scripts are loaded only from
the overlay's `conf/scripts.toml`, not public `conf/`, and become explicitly
configured dynamic tasks.

The [script workflow](../cli/src/domains/overlay/scripts.rs) calls check and
preview modes, including `--check` and `--dryrun`. These are cooperative
contracts: a script can ignore them, print private data, access the network or
mutate anything its process can access. A dry-run is **not a sandbox**.
APM fragments and agent/editor content likewise cross into tools that interpret
them; review the deployed content, not only the manifest shape.

Path validation protects particular managed-resource boundaries, such as
relative home-link targets and system-file targets below `/etc`. It is not a
general filesystem confinement policy for every external command or script.

## Secrets

Keep credentials, private keys, tokens, connection strings and private
machine/overlay values out of tracked configuration, fixtures, examples,
workflow files and diagnostic uploads.

The [pre-commit scanner](HOOKS.md#scan-scope) checks added staged text against
versioned regex rules, prints redacted matches, and supports narrow safe-span
exceptions. It is local and bypassable, does not scan all history or arbitrary
binary/encoded data, and trusts the checkout's helper code and rules. Neither a
clean scan nor a green CI result establishes that a patch is secret-free.

### Logs are a separate disclosure boundary

[`CommandSpec`](../cli/src/infra/exec/mod.rs) controls two independent things:

- `redact_arguments()` suppresses command arguments in diagnostics.
- `OutputLog::Omit` excludes captured streams from persisted command diagnostics
  and checked-command errors; it does not sanitize values a caller later logs.

By default, failed stdout/stderr and successful stderr are retained; successful
stdout gets a byte count. `OutputLog::Full` retains successful stdout too.
Domain messages and overlay-script output can also reach logs. There is no
universal secret scrubber. Inspect logs locally and share only the smallest
sanitized excerpt, even when a console failure looks innocuous.

If a real secret reaches a commit or shared log, revoke/rotate it first. Removing
the latest occurrence does not invalidate copies, history, artifacts or logs.
Coordinate any subsequent history cleanup separately rather than assuming a
force-push can undo exposure.

## Dependency and CI controls

[`ci.yml`](../.github/workflows/ci.yml) runs input-selected dependency
advisory/policy checks and builds/tests. Its `ci-success` gate requires every
selected job to succeed and only unselected jobs to be skipped; classification
failure or an unexpected skip fails the gate. Coverage and mutation reports are
informational. See
[CI gates](TESTING.md#ci-gates) for exact coverage instead of treating “CI green”
as a universal security claim.

[`release.yml`](../.github/workflows/release.yml) has a narrower publishing
boundary:

1. Accept only a successful CI workflow from a same-repository **push to main**.
2. With read-only permissions, compare the exact tested SHA against the latest
   published non-draft, non-prerelease release. Select publication only when
   binary/publishing inputs changed, or no release exists.
3. Carry that workflow's exact tested SHA into release builds and the tag.
4. Serialize release runs without cancelling an active release.
5. Build without dependency caches in the release workflow.
6. Grant contents/OIDC/attestation write permissions to the publishing job,
   generate checksums and attestations, verify discoverability, then publish.

The published-release baseline retains pending binary changes after failed or
cancelled publishing runs; a later docs-only push can therefore finish an
unpublished release. Older/already-released commits skip publication, while API
errors, missing release tags or divergent history fail selection. See
[Release selection](TESTING.md#release-selection) for the input policy and
isolated regression checks.

There is no manual dispatch path selecting an arbitrary source revision.
These guards reduce accidental or untrusted-event publication; they do not
protect against a trusted maintainer account, dependency or approved workflow
being compromised.

## Safe contribution practices

For security-relevant changes:

- Review trust-boundary changes explicitly: new executable content, external
  sources, privileged commands and data retained in logs.
- Pin external Actions to full commit SHAs and keep permissions scoped to the
  job that needs them. Version comments are for readability, not pinning.
- Use checked subprocesses or explicitly handle nonzero results; do not turn
  validation/discovery failures into success-shaped defaults.
- Preserve idempotency and dry-run tests, but do not describe them as isolation
  or rollback.
- Test with synthetic secrets and isolated paths, not private overlay excerpts.

Use [Contributing](CONTRIBUTING.md) for the change workflow and
[Testing](TESTING.md) for safe validation.

## Reporting a vulnerability

Do not post credentials, private configuration or sensitive exploit details in
a public issue. Check the repository's current
[Security page](https://github.com/sneivandt/dotfiles/security) for any available
private reporting mechanism before sending details. This document does not
assert that private reporting is enabled or promise a response process.
If no private channel is advertised, ask how to contact the maintainer without
including the sensitive material.
