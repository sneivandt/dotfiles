# Security Model

This document describes the repository's security controls and trust
boundaries. It is not a public vulnerability-disclosure policy.

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

A checksum proves that the binary matches the published release metadata; it
does not independently establish who produced that release. Repository and
GitHub account security remain part of the trust chain.

Use wrapper `--build` when you need the binary compiled from the local checkout.

## Elevation

Tasks plan elevation before applying operations that require it. The default
should remain least privilege:

- Windows symlinks use Developer Mode where possible.
- Registry settings are currently user-scoped.
- systemd configuration uses user units.
- system-level WSL configuration may require elevation.
- package managers elevate only for provider actions that need it.

Do not move broad task execution behind an unconditional administrator or root
requirement.

## Private overlays

Overlays are explicitly supplied local repositories. They may contain private
desired state and executable scripts. The public repository:

- appends supported overlay configuration
- resolves overlay symlink sources from the overlay root
- executes only scripts listed in the overlay's `conf\scripts.toml`
- does not load scripts from its own public `conf\`

Review an overlay before using `--overlay`; dry-run reduces mutation risk but
does not make an untrusted executable safe to inspect or invoke. The engine
passes `--check` and `--dryrun` to opaque scripts but cannot prevent mutation
when a script violates that convention.

## Secrets

Do not place credentials, private keys, tokens, connection strings, or
machine-specific secret values in:

- `conf\`
- `symlinks\`
- test fixtures
- logs
- documentation examples
- GitHub workflow files

The pre-commit hook scans staged content using `hooks\sensitive-patterns.ini`.
This is defense in depth, not a guarantee. Generated command output and overlay
script output are logged, so scripts must avoid printing sensitive values.

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
`main`, before jobs receive write permissions or publishing secrets. Release
assets include checksums consumed by the wrappers.

## Safe contribution practices

- Pin or constrain external actions and tooling according to repository policy.
- Keep workflow permissions least-privilege.
- Never echo secrets in shell tracing or diagnostic logs.
- Preserve dry-run semantics for every mutation.
- Propagate validation failures instead of falling back to unsafe defaults.
- Avoid following unvalidated paths outside the expected repository, home, or
  configuration roots.
- Review package, APM, and overlay supply-chain changes separately from code
  correctness.

## Reporting a vulnerability

Do not include exploit details or credentials in a public issue. Use the
repository host's private security-reporting mechanism when enabled, or contact
the repository owner privately through an established trusted channel.
