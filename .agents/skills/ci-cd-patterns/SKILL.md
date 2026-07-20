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
| `release.yml` | Successful CI `workflow_run` from same-repo push to main | Build release binaries, create GitHub Release |
| `docker.yml` | Successful CI `workflow_run` from same-repo push to main | Build and push Docker image to Docker Hub |

## CI Pipeline (`ci.yml`)

Current gating areas include formatting, script linting, config validation,
`cargo-audit`, `cargo-deny`, Linux/Windows builds, the MSRV check, integration
tests, install/uninstall tests, application tests, hook tests, and wrapper tests.
The `coverage` job is informational (`continue-on-error`) and intentionally does
not gate `ci-success`.

Maintain these invariants:

- Keep workflow permissions least-privilege.
- Use concurrency cancellation for superseded runs.
- Use `--profile ci` for CI builds and tests; reserve `--release` for publishing.
- Upload build artifacts for downstream integration jobs rather than rebuilding.
- Keep `ci-success` on `if: always()` and list every **gating** job in `needs`.
- Do not add informational jobs such as coverage to the required gate unless
  intentionally making them blocking.

Integration logic belongs in `.github/workflows/scripts/linux/` and
`.github/workflows/scripts/windows/`, with shared shell helpers under
`scripts/linux/lib/`. Prefer scripts over large inline workflow steps, and run
dotfiles integration cases against the checkout with `--root .`.

For CI-profile reproduction:

```bash
cargo test --profile ci --manifest-path cli/Cargo.toml
cargo clippy --profile ci --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift
```

Use `cross-platform-verification` for the canonical general Rust and wrapper
sequence.

## Publishing Workflows

`release.yml` and `docker.yml` consume `workflow_run`. Their initial guard must
verify that the completed CI run:

- succeeded
- came from a push to `main`
- came from the same repository

Apply the guard before jobs that receive write permissions or secrets. Release
artifacts must retain the wrapper-expected names and publish SHA-256 checksums.
Docker publishing must check out the exact successful CI head SHA.

## Change Checklist

1. Add the job to `ci.yml` with appropriate `needs:` dependencies
2. Decide explicitly whether it is gating or informational
3. Add gating jobs to `ci-success.needs`
4. Use `fail-fast: false` for independent matrix cases
5. Put recurring test logic in the platform script directories
6. Download existing artifacts when the job needs the compiled binary
7. Add the narrowest practical local guard for recurring CI-only failures
