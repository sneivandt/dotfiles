---
name: ci-cd-patterns
description: >
  Use for .github/workflows/, CI guard scripts, release tags/assets, action
  pinning, ci-success gating, or monitoring CI after a push. Not for ordinary
  local test selection or investigating/fixing mutation test runs (use
  mutation-testing).
---

# CI/CD Patterns

Treat workflow files as authoritative; do not duplicate their full job or
action-version inventories in guidance.

## CI rules

- Keep permissions least-privilege. Let active CI runs on `main` finish and
  keep only the newest pending run. Cancel superseded CI runs on other refs.
  Preserve the [release workflow's](../../../.github/workflows/release.yml)
  non-cancelling concurrency policy so publishing can finish.
- Pin external actions to full commit SHAs with the release tag in a comment.
- Use the `ci` Cargo profile for checks and `release` only for publishing.
- Put recurring logic in `.github/workflows/scripts/{linux,windows}/`.
- Upload build artifacts for downstream tests instead of rebuilding.
- Prefer extending an existing CI job over adding a new standalone job when
  the coverage fits naturally, and keep shard concurrency and runner overhead
  lean so background runs leave capacity for normal CI.
- `ci-success` runs under `if: always()` and depends on every gating job.
  Informational jobs stay out unless deliberately promoted.
- Update path classification alongside job conditions and dependencies. A new
  job that is always skipped is not coverage; preserve docs-only execution and
  distinguish intended skips from failures/cancellation in the success gate.
- Keep local `check.sh` and `Check.ps1` stages aligned with gating concerns.
- Keep mutation testing in the separate nightly/manual
  [workflow](../../../.github/workflows/mutation.yml), outside `ci-success` and
  release/Pages dependencies. A failed mutation run must stay visibly failed,
  retain its reports, and not advance the nightly comparison baseline. Manual
  replay ranges must not narrow later scheduled coverage, and mutation helper
  scripts retry transient GitHub API errors.
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
  deployed that commit's tested artifact (retries reuse the packaged artifact
  rather than rebuilding); for a release verify the tag's target,
  expected assets, and checksums. A deliberate publication skip is valid; a
  pending or failed publisher is not completion. Check deployed content when
  the change affects it; routine wording does not need another browser audit.
- Wait for completion, inspect failed job logs, and report actionable failures.
  If monitoring is unavailable, state the SHA, run, and blocker.
- If a newer push replaces the requested SHA's pending run, report it as
  cancelled and verify whether the newer SHA contains the requested commits.
  If it does, follow that SHA's checks and any applicable publication, and
  report those results under the newer SHA.
- Do not launch or wait for nightly mutations merely because a push changed
  Rust. Use [mutation-testing](../mutation-testing/SKILL.md) when investigating
  failures, changing mutation selection/exclusions, or replaying `mutation_base`
  on `mutation.yml`.

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
