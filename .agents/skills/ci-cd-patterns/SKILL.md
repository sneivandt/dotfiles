---
name: ci-cd-patterns
description: >
  CI/CD pipeline structure, release workflow, and integration test scripts.
  Use when modifying GitHub Actions workflows, adding CI jobs, or changing
  the release/binary distribution process.
---

# CI/CD Patterns

Use this skill for workflow topology, CI-only reproduction, and publishing
changes. Treat `.github/workflows/*.yml` as authoritative; avoid copying action
versions or complete job inventories into guidance.

## Workflow Boundaries

| Workflow | Trigger | Purpose |
|---|---|---|
| `ci.yml` | Push/PR to main | Build, lint, test, integration checks |
| `release.yml` | Successful CI `workflow_run` from same-repo push to main, or `workflow_dispatch` | Resolve the release version, build release binaries, create GitHub Release |
| `docker.yml` | Successful CI `workflow_run` from same-repo push to main | Build and push Docker image to Docker Hub |

## CI Pipeline (`ci.yml`)

Current gating areas include formatting, script linting, config validation,
`cargo-audit`, `cargo-deny`, Linux/Windows builds, the MSRV check, integration
tests, install/uninstall tests, application tests, hook tests, and wrapper tests.
The `coverage` job is informational (`continue-on-error`) and intentionally does
not gate `ci-success`.

Maintain these invariants:

- Keep workflow permissions least-privilege.
- Pin every external action to a full commit SHA with the tag in a trailing
  comment. Moving tool tags (for example `taiki-e/install-action@cargo-deny`)
  become a pinned SHA plus an explicit `with: tool:` input.
- Use concurrency cancellation for superseded runs.
- Use `--profile ci` for CI builds and tests; reserve `--release` for publishing.
- Upload build artifacts for downstream integration jobs rather than rebuilding.
- Keep `ci-success` on `if: always()` and list every **gating** job in `needs`.
- Do not add informational jobs such as coverage to the required gate unless
  intentionally making them blocking.

Integration logic belongs in `.github/workflows/scripts/linux/` and
`.github/workflows/scripts/windows/`, with shared shell helpers under
`.github/workflows/scripts/linux/lib/`. Prefer scripts over large inline
workflow steps, and run dotfiles integration cases against the checkout with
`--root .`.

`.github/workflows/scripts/linux/check.sh` and its PowerShell twin
`.github/workflows/scripts/windows/Check.ps1` run the gating checks locally
under the same `ci` profile. Keep them in step when adding or removing a gating
CI concern, and prefer extending a stage over adding a parallel script.

For CI-profile reproduction:

```bash
sh .github/workflows/scripts/linux/check.sh
cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift
```

Use `cross-platform-verification` for the canonical general Rust and wrapper
sequence.

## Release versioning

Release tags are `vYYYY.MM.DD-N`, where `N` starts at 1 and increments for each
additional release on the same day. A dedicated `version` job resolves the tag
exactly once — probing existing remote tags to pick `N` — and exposes it plus
the resolved commit SHA as outputs. Every downstream build and publish job
consumes those outputs rather than recomputing a version, so all assets in a
release agree.

`cli/Cargo.toml`'s `version` field is crate metadata, not the release version.

The format is load-bearing: `cli/src/domains/dotfiles/self_update/version.rs`
parses release tags to decide whether an update is newer, and accepts three
dot-separated date components plus the hyphen-separated increment. Changing the
tag shape requires changing that parser in the same commit, and doing so strands
already-installed binaries, which can no longer parse the new tags.

## Publishing Workflows

`release.yml` and `docker.yml` consume `workflow_run`. Their initial guard must
verify that the completed CI run:

- succeeded
- came from a push to `main`
- came from the same repository

Apply the guard before jobs that receive write permissions or secrets. Release
artifacts must retain the wrapper-expected names, publish SHA-256 checksums, and
carry build provenance attestations (`id-token: write` plus `attestations:
write` on the publishing job). Docker publishing must check out the exact
successful CI head SHA.

Download paths (`dotfiles.sh`, `dotfiles.ps1`, and the Rust self-update task)
verify the checksum first and then the provenance attestation through the `gh`
CLI. Provenance verification is advisory by default and is controlled by
`DOTFILES_SKIP_ATTESTATION` and `DOTFILES_REQUIRE_ATTESTATION`; keep wrapper
integration tests hermetic by stubbing `gh` or setting
`DOTFILES_SKIP_ATTESTATION=1`.

## Change Checklist

1. Add the job to `ci.yml` with appropriate `needs:` dependencies
2. Decide explicitly whether it is gating or informational
3. Add gating jobs to `ci-success.needs`
4. Use `fail-fast: false` for independent matrix cases
5. Put recurring test logic in the platform script directories
6. Download existing artifacts when the job needs the compiled binary
7. Add the narrowest practical local guard for recurring CI-only failures
