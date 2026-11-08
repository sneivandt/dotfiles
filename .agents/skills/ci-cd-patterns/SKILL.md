---
name: ci-cd-patterns
description: >
  Use for .github/workflows/, CI guard scripts, release tags/assets, action
  pinning, ci-success gating, or monitoring CI after a push. Not for ordinary
  local test selection.
---

# CI/CD Patterns

Treat workflow files as authoritative; do not duplicate their full job or
action-version inventories in guidance.

## CI rules

- Keep permissions least-privilege and cancel superseded CI runs. Preserve the
  [release workflow's](../../../.github/workflows/release.yml) non-cancelling
  concurrency policy so publishing is not interrupted.
- Pin external actions to full commit SHAs with the release tag in a comment.
- Use the `ci` Cargo profile for checks and `release` only for publishing.
- Put recurring logic in `.github/workflows/scripts/{linux,windows}/`.
- Upload build artifacts for downstream tests instead of rebuilding.
- `ci-success` runs under `if: always()` and depends on every gating job.
  Informational jobs stay out unless deliberately promoted.
- Update path classification alongside job conditions and dependencies. A new
  job that is always skipped is not coverage; preserve docs-only execution and
  distinguish intended skips from failures/cancellation in the success gate.
- Keep local `check.sh` and `Check.ps1` stages aligned with gating concerns.
- A confirmed local npm TLS failure need not block a push. Use the pre-push
  checker's [unavailable-coverage receipt](../../../docs/content/development/testing.md#local-npm-tls-failures)
  for the exact commit, run the remaining selected checks, and report docs as
  unavailable, not passed. Other failures remain blocking, TLS validation stays
  enabled, and the exact commit's CI documentation job must still pass.

## After pushing

- Treat a user request to push as including CI monitoring unless they explicitly
  opt out. Do not stop at a successful Git push or wait for another request.
- Find the CI run for the exact pushed SHA and verify the aggregate gate and
  other checks, including CodeQL when selected. Use run/job status to wait at
  sensible intervals; fetch logs for failures and artifacts when needed, rather
  than repeatedly downloading successful logs or rerunning local suites.
- Follow selected Pages and release workflows after aggregate CI. Verify Pages
  deployed that commit's tested artifact; for a release verify the tag's target,
  expected assets, and checksums. A deliberate publication skip is valid; a
  pending or failed publisher is not completion. Check deployed content when
  the change affects it; routine wording does not need another browser audit.
- Wait for completion, inspect failed job logs, and report actionable failures.
  If monitoring is unavailable, state the SHA, run, and blocker. A later push's
  green run does not establish that the requested commit passed.
- Mutation shards are required when selected. Inspect every shard report for
  survivors, timeouts, and tooling errors, and fix the cause before reporting
  success. Keep the exact matrix and use narrowly justified exclusions for
  equivalent or platform-only mutations.
- Push recovery compares against a successful ancestor so a test-only follow-up
  still checks pending Rust changes. For manual replay, pass the original ancestor
  through `mutation_base` and verify that the run selected the shards.

## Release rules

- Release tags are `vYYYY.MM.DD-N`.
- Resolve the version and source SHA once; every build/publish job consumes
  those outputs.
- Changing the tag shape also requires updating the self-update parser and
  considering already-installed clients.
- `workflow_run` publishers must verify successful same-repository pushes to
  `main` before granting write permissions or secrets.
- Compare binary inputs with the latest published release, not the previous
  push, so a failed or superseded publish does not lose pending changes.
- Do not execute untrusted PR code in a privileged publishing context or
  interpolate untrusted event text into shell source. Pass data through
  environment variables/arguments and validate it.
- Preserve wrapper-expected asset names, checksums, and provenance attestations.

## Change checklist

1. Decide whether a new job is gating or informational.
2. Declare its dependencies and update `ci-success` if gating.
3. Reuse artifacts and platform scripts.
4. Add the narrowest local guard for a recurring CI-only failure, including
   affected and unaffected paths when changing classification.

Start with [CI](../../../.github/workflows/ci.yml) and
[change classification](../../../.github/workflows/scripts/linux/classify-ci-changes.py)
(the `.sh` entry point delegates to it).
Use [Testing](../../../docs/content/development/testing.md) for canonical commands and the current CI
coverage matrix.
