---
name: ci-cd-patterns
description: >
  Use for .github/workflows/, CI guard scripts, release tags/assets, Docker
  publishing, action pinning, or ci-success gating. Not for ordinary local test
  selection.
---

# CI/CD Patterns

Treat workflow files as authoritative; do not duplicate their full job or
action-version inventories in guidance.

## CI rules

- Keep permissions least-privilege and cancel superseded runs.
- Pin external actions to full commit SHAs with the release tag in a comment.
- Use the `ci` Cargo profile for checks and `release` only for publishing.
- Put recurring logic in `.github/workflows/scripts/{linux,windows}/`.
- Upload build artifacts for downstream tests instead of rebuilding.
- `ci-success` runs under `if: always()` and depends on every gating job.
  Informational jobs stay out unless deliberately promoted.
- Keep local `check.sh` and `Check.ps1` stages aligned with gating concerns.

## Release rules

- Release tags are `vYYYY.MM.DD-N`.
- Resolve the version and source SHA once; every build/publish job consumes
  those outputs.
- Changing the tag shape also requires updating the self-update parser and
  considering already-installed clients.
- `workflow_run` publishers must verify successful same-repository pushes to
  `main` before granting write permissions or secrets.
- Preserve wrapper-expected asset names, checksums, and provenance attestations.
- Docker publishing checks out the exact successful CI head SHA.

## Change checklist

1. Decide whether a new job is gating or informational.
2. Declare its dependencies and update `ci-success` if gating.
3. Reuse artifacts and platform scripts.
4. Add the narrowest local guard for a recurring CI-only failure.

Use `cross-platform-verification` for normal local Rust/wrapper checks.
