# Security model

This guide describes the repository's trust boundaries and security controls.
It is not a public vulnerability-disclosure policy.

## Trust boundaries

The CLI can modify user and machine state. Treat these inputs as trusted code or
trusted configuration:

- the checked-out repository
- a path supplied through `--overlay`
- release assets downloaded by a wrapper
- commands executed by package providers
- overlay scripts
- APM packages and local plugins

Review changes to these inputs before applying them, particularly when elevation
may be required.

## Release downloads

When the platform binary is absent, the wrappers download a published release
asset and its checksum. They:

1. Select the asset for the detected operating system and architecture.
2. Use HTTPS for GitHub release access.
3. Download the corresponding SHA-256 checksum.
4. Verify the binary before executing it.
5. Verify the binary's GitHub build provenance attestation when `gh` is
   available.

The same sequence applies to the binary's own self-update download.

A checksum proves that the binary matches the published release metadata, but
not who produced the release. The release workflow also publishes a provenance
attestation for each asset. The attestation identifies the workflow, repository,
and commit that produced the binary. Repository and GitHub account security
remain part of the trust chain.

## Build provenance verification

Provenance verification uses the `gh` CLI (`gh attestation verify`). During
initial wrapper bootstrap, a missing `gh` command produces a warning and skips
the attestation check so the CLI can install its configured packages. A present
`gh` command that cannot verify the attestation still fails the bootstrap.
Self-update keeps its existing verification policy.

| Environment variable | Effect |
|---|---|
| unset | Verify when `gh` is available; warn and continue when it is absent during wrapper bootstrap |
| `DOTFILES_SKIP_ATTESTATION=1` | Skip provenance verification entirely |

An unverifiable self-update leaves the currently installed binary unchanged.
Verification can be performed manually after bootstrap:

```sh
gh attestation verify bin/dotfiles --repo sneivandt/dotfiles
```

Use wrapper `--build` when you need the binary compiled from the local checkout.

## Elevation

Tasks plan elevation before applying operations that require it. Keep elevation
to the smallest necessary scope:

- Windows symlinks use Developer Mode where possible.
- Registry settings are currently user-scoped.
- systemd configuration defaults to user units; explicitly scoped system units
  may require elevation.
- system-level WSL configuration may require elevation.
- package managers elevate only for provider actions that need it.

Do not move broad task execution behind an unconditional administrator or root
requirement.

Elevation is scoped to the tasks that declare it. The main process stays
unprivileged on both platforms:

- Linux primes `sudo` once and runs only the privileged commands under it.
- Windows delegates the elevating tasks to one short-lived elevated child run
  restricted to those selectors, then continues unprivileged. The child cannot
  recurse into another elevated run.

The CLI never requests elevation in a non-interactive or CI session. If
elevation is declined or unavailable, it skips the affected tasks and their
dependents instead of aborting the whole run.

## Private overlays

Overlays are explicitly supplied local repositories. They may contain private
desired state and executable scripts. The public repository:

- appends supported overlay configuration
- resolves overlay symlink sources from the overlay root
- executes only scripts listed in the overlay's `conf/scripts.toml`
- does not load scripts from its own public `conf/`

Review an overlay before using `--overlay`. A dry run reduces mutation risk, but
it does not make an untrusted executable safe. The engine passes `--check` and
`--dryrun` to scripts but cannot stop a script that ignores those flags from
changing state.

## Secrets

Do not place credentials, private keys, tokens, connection strings, or
machine-specific secret values in:

- `conf/`
- `symlinks/`
- test fixtures
- logs
- documentation examples
- GitHub workflow files

The pre-commit hook scans staged content using `hooks/sensitive-patterns.ini`.
The scan is a backup control, not a guarantee. Generated command output and
overlay script output enter the logs, so scripts must not print sensitive
values.

If a secret is committed, revoke or rotate it first; removing it from the latest
commit is not sufficient.

## Dependency and CI controls

CI includes:

- Cargo dependency auditing
- Cargo policy and license checks
- formatting and linting
- Linux and Windows builds/tests
- wrapper and integration behavior
- publishing guard checks

Publishing workflows run only after successful CI from a same-repository push to
`main`, before jobs receive write permissions or publishing secrets. Manual
release dispatches must identify a completed, successful CI push run with the
same provenance, and all release runs are serialized to prevent tag-allocation
races. Release assets include checksums and build provenance attestations
consumed by the wrappers and the self-update path.

## Safe contribution practices

- Pin every external GitHub Action to a full commit SHA, with the human-readable
  tag kept in a trailing comment. Moving tags (including tool tags such as
  `taiki-e/install-action@cargo-deny`) must be expressed as a pinned SHA plus an
  explicit input.
- Keep workflow permissions least-privilege.
- Never echo secrets in shell tracing or the run log.
- Preserve dry-run semantics for every mutation.
- Propagate validation failures instead of falling back to unsafe defaults.
- Avoid following unvalidated paths outside the expected repository, home, or
  configuration roots.
- Review package, APM, and overlay supply-chain changes separately from code
  correctness.

## Reporting a vulnerability

Do not include exploit details or credentials in a public issue. Report the
vulnerability privately through
[GitHub's vulnerability reporting form](https://github.com/sneivandt/dotfiles/security/advisories/new).
