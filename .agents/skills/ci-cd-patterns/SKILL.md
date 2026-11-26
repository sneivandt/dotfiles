---
name: ci-cd-patterns
description: >
  Use for .github/workflows/, CI guard scripts, release tags/assets, action
  pinning, ci-success gating, or monitoring CI after a push. Not for ordinary
  local test selection or investigating/fixing mutation test runs (use
  mutation-testing).
---

# CI/CD Patterns

Workflows ([ci.yml](../../../.github/workflows/ci.yml),
[release.yml](../../../.github/workflows/release.yml),
[pages.yml](../../../.github/workflows/pages.yml),
[mutation.yml](../../../.github/workflows/mutation.yml)) and
[classify-ci-changes.py](../../../.github/workflows/scripts/linux/classify-ci-changes.py)
are authoritative; see [Testing](../../../docs/content/development/testing.md)
for commands and coverage matrix.

## Workflow contracts

- Pin external actions to full commit SHAs with the release tag in a comment.
  Keep recurring CI/orchestration logic in `.github/workflows/scripts/{linux,windows}/`
  and application/component/integration tests under `tests/`; extend existing
  jobs before adding new ones.
- Concurrency: let active `main` CI runs finish and keep only the newest
  pending run; cancel superseded runs on other refs; preserve non-cancelling
  concurrency on `release.yml` and `pages.yml`.
- Use the `ci` Cargo profile for checks and `release` only for publishing;
  reuse uploaded build artifacts across downstream jobs.
- `ci-success` runs under `if: always()` and depends on every gating job; keep
  informational jobs and nightly `mutation.yml` out of it. Update
  `classify-ci-changes.py` alongside job conditions so docs-only runs stay
  scoped and intended skips stay distinct from failures/cancellation.
- Keep `mutation.yml` visibly failed on survivor/shard errors, retaining
  reports and never advancing the nightly baseline on failure or manual replay.
- For a confirmed local npm TLS failure only, use the pre-push
  [unavailable-coverage receipt](../../../docs/content/development/testing.md#local-npm-tls-failures)
  for the exact commit, run all other selected checks, report docs as
  unavailable (not passed), and verify the exact commit's CI docs job passes.

## After pushing

- A push request includes monitoring the exact pushed SHA's aggregate CI (and
  CodeQL when selected) through completion unless the user opts out.
- After aggregate CI passes, follow selected `pages.yml` and `release.yml`
  runs: verify Pages deployed that commit's tested artifact (deliberate skip is
  valid), and for releases verify tag target, expected assets, and checksums.
- If a newer push cancels the requested SHA's run and contains the requested
  commits, report the cancellation and follow the newer SHA through completion.
- If monitoring is blocked, report the exact SHA, run ID, and blocker. Do not
  launch or wait for nightly `mutation.yml` on ordinary Rust pushes.

## Release and publisher security

- Release tags are `vYYYY.MM.DD-N`. Resolve version and source SHA once and
  consume those outputs across all jobs; tag-shape changes also affect
  [self-update](../self-update/SKILL.md) and already-installed clients.
- `workflow_run` publishers must verify successful same-repository pushes to
  `main` before granting write permissions or secrets.
- Compare binary inputs against the latest published release, not the previous
  push, so failed/superseded publishes cannot lose pending changes.
- Preserve wrapper-expected asset names, checksums, and provenance attestations.
  Never run untrusted PR code in a privileged publisher or interpolate untrusted
  event text into shell code.
