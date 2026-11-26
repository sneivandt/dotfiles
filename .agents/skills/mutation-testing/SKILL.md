---
name: mutation-testing
description: >
  Use when investigating or fixing failed Mutation Testing workflow runs,
  triaging surviving mutants, timeouts, or equivalent-mutant exclusions,
  running focused local mutations with select-mutations.py or
  run-mutations.py, or replaying a mutation range. Not for ordinary local
  test selection, general Rust test-harness design, or unrelated CI workflow
  gating and releases (use ci-cd-patterns).
---

# Mutation Testing

Exact local commands live in
[Mutation testing](../../../docs/content/development/mutation-testing.md), and
the risk-based selection table for ordinary Rust work lives in
[Focused mutation testing](../../../docs/content/development/testing.md#focused-mutation-testing).

## Inspect a failed CI run

1. Find the run in [mutation.yml](../../../.github/workflows/mutation.yml)
   (`gh run list -w mutation.yml`) and download `mutation-summary` (plus
   `mutation-inputs` when `mutation-plan.json` is needed) into a temp directory
   before reading shard logs. The summary includes the `base -> head` range,
   counts, missing/failed shards, and `--exact` replay commands.
2. Fetch `rust-mutation-results-*` shard artifacts only when a shard timed out,
   crashed, or omitted details.

## Triage rules

- **`MISSED` real behavior**: assert the user/caller-observable outcome at the
  lowest layer (result plus side effect or preserved state).
- **`MISSED` equivalent mutant**: prefer simplifying redundant code; otherwise
  add a justified narrow regex to `LINUX_EXCLUSIONS` or `WINDOWS_EXCLUSIONS` in
  [run-mutations.py](../../../.github/workflows/scripts/linux/run-mutations.py).
- **Cross-platform alignment**: keep `NATIVE_MODULES`, `LINUX_EXCLUSIONS`,
  `WINDOWS_FILES`, and `WINDOWS_PATTERN` aligned in `run-mutations.py` so Linux
  exclusions never silently drop Windows coverage.
- **`TIMEOUT`**: never treat a timeout as caught—fix slow/hanging tests or route
  blocking mutations to a bounded target in `FOCUSED` in `run-mutations.py`.

## Local replay pitfalls

- `run-mutations.py` runs `cargo mutants --in-place` on the native OS
  filesystem; keep the working tree clean before invoking it because an
  interrupted run can leave mutant edits in `git diff`, and copy any report you
  want to keep out of `cli/mutants.out` before starting another run.
- [select-mutations.py](../../../.github/workflows/scripts/linux/select-mutations.py)
  selects inputs against committed `HEAD` (not uncommitted source edits). Pass
  the run's `ORIGINAL_BASE_SHA` (or `--all-source` when no passing baseline
  existed). Never replace `ORIGINAL_BASE_SHA` with a later commit for a
  test-only fix, and never count a zero-mutant run as evidence.
- Clean up generated `cli/mutants.diff`, `cli/mutation-plan.json`, and
  `cli/mutants.out*` that you created before committing.

## CI cadence

- Only a passing **scheduled** run on `main` advances the nightly baseline
  (`event=schedule&status=success`); manual dispatches verify a range without
  moving it.
- Iterate locally first. Dispatch `mutation.yml` with
  `mutation_base=<ORIGINAL_BASE_SHA>` only when remote or native Windows
  verification is needed (never replay on `ci.yml`), and monitor through
  **Mutation: Results** for zero survivors, timeouts, and shard errors.
