---
title: Mutation testing
description: Check whether tests detect incorrect Rust behavior using a committed source range.
---

# Mutation testing

Mutation testing introduces temporary faults into Rust source to verify that
tests catch flipped branches, dropped errors, and broken previews. Run it
locally when it adds meaningful confidence for safety-critical or failure-path
changes, or when fixing a surviving mutant; see the
[decision table](testing.md#focused-mutation-testing) for when to use it.

When a mutant reports `MISSED`, add a test assertion that passes on the real
code and fails on the mutation (or verify the mutant is equivalent). Survivors,
timeouts, and tooling errors all fail the run. Neither normal `cargo test` nor
`check.py --all` runs mutations, and the
[nightly workflow](ci-and-releases.md#scheduled-mutation-testing) runs
independently of push CI and releases.

## Prerequisites and baseline

Install `cargo-mutants` 27.1.0 to match CI. Installing `cargo-nextest` (0.9.152
in CI) is optional locally and enables fail-fast test runs automatically when
found on `PATH` (pass `--test-tool cargo` to force `cargo test`). Use the
pinned Rust toolchain on the native filesystem for the OS under test, since
Linux and Windows mutations exercise different runtime contracts.

The runner compiles under the incremental `[profile.mutants]` profile so
repeated in-place edits reuse unchanged crates.

For a hosted nightly failure, open the **Mutation: Results** summary (or
download its `mutation-summary` artifact), note its base and tested SHAs, and
copy the focused replay commands. After fixing a survivor, rerun against the
same `ORIGINAL_BASE_SHA` even if you only edited tests.

For a local change not yet in CI, commit your source and tests and use the
base SHA from `pre-push.py --plan` (see
[Implementation and mixed patches](testing.md#implementation-and-mixed-patches)).

Start from a clean working tree so any leftover edit from an interrupted
in-place run is easy to spot with `git diff`. If you need to reproduce an older
tested commit without switching your current checkout, you can optionally run
these commands in a separate detached Git checkout.

## Select and run the mutations

From the repository root, select the affected mutants using the recorded full
`ORIGINAL_BASE_SHA` (or `--all-source` if the hosted report noted no passing
baseline):

```sh
python3 -B .github/workflows/scripts/linux/select-mutations.py --base ORIGINAL_BASE_SHA
```

On Linux, paste the focused replay command from the report to check one mutant,
or drop `--exact` and its argument to replay the whole slice. For example, to
replay process-execution mutants with their bounded test target:

```sh
python3 -B .github/workflows/scripts/linux/run-mutations.py run process
```

Use `run linux` for the full Linux slice, or `run linux-lib` / `run linux-app`
for just the library (`--lib`) or CLI/integration slice.

On Windows, use `python` for selection and run the native Windows suite on a
Windows filesystem:

```powershell
python -B .github/workflows/scripts/linux/run-mutations.py run windows
```

Because replay commands mutate files in place, inspect `cli/mutants.out` for
survivors, timeouts, and tool errors and copy out anything you want to keep
before starting another run (which rotates that directory). If a focused replay
says the mutation description changed, re-select from the same
`ORIGINAL_BASE_SHA` and rerun the broader slice.

## Clean up generated artifacts

Selection and replay write untracked files under `cli/` (`cli/mutants.diff`,
`cli/mutation-plan.json`, and `cli/mutants.out*`). Before committing, check
that no interrupted mutation edit remains and remove the generated artifacts:

```sh
git status --short
git diff
rm -rf cli/mutants.diff cli/mutation-plan.json cli/mutants.out cli/mutants.out.old
```

For how scheduled runs accumulate diffs, shard work, and exclude native code,
continue to [CI and releases](ci-and-releases.md#scheduled-mutation-testing).
