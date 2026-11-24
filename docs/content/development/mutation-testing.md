---
title: Mutation testing
description: Check whether tests detect incorrect Rust behavior using a committed source range and a disposable worktree.
---

# Mutation testing

Mutation testing introduces temporary faults into Rust source to verify that
tests catch flipped branches, dropped errors, and broken previews. Run it
locally when it adds meaningful confidence for safety-critical or failure-path
changes, or when fixing a surviving mutant; see the
[decision table](testing.md#focused-mutation-testing) for when to use it.

Always run `cargo-mutants` inside a disposable Git worktree rather than your
main working tree. When a mutant reports `MISSED`, add a test assertion that
passes on the real code and fails on the mutation (or verify the mutant is
equivalent). Survivors, timeouts, and tooling errors all fail the run. Neither
normal `cargo test` nor `check.sh --all` runs mutations, and the
[nightly workflow](ci-and-releases.md#scheduled-mutation-testing) runs
independently of push CI and releases.

## Prepare an isolated source snapshot

Install `cargo-mutants` 27.1.0 to match CI. Installing `cargo-nextest` (0.9.152
in CI) is optional locally and enables fail-fast test runs automatically when
found on `PATH` (pass `--test-tool cargo` to force `cargo test`). Use the
pinned Rust toolchain on the native filesystem for the OS under test, since
Linux and Windows mutations exercise different runtime contracts.

The runner compiles under the incremental `[profile.mutants]` profile so
repeated in-place edits reuse unchanged crates.

For a hosted nightly failure, open the **Mutation: Results** summary, note its
base and tested SHAs, and copy the focused replay command. Reproduce the
survivor at the tested commit first; after committing your fix, rerun the full
selection from the same base SHA against your new commit (even if you only
edited tests).

For a local change not yet in CI, commit your source and tests and use the
base SHA from `pre-push.py --plan` (see
[Implementation and mixed patches](testing.md#implementation-and-mixed-patches)).

Create a detached disposable worktree from the repository root (substituting
the tested SHA for `HEAD` when reproducing a hosted failure):

```sh
git worktree add --detach .mutation-review HEAD
```

Only committed files appear in the worktree; keep it and its reports outside
your patch.

## Select and run the mutations

From the root of `.mutation-review`, select the affected mutants using the
recorded full `ORIGINAL_BASE_SHA` (or `--all-source` if the hosted report noted
no passing baseline):

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
says the mutation description changed, rerun the broader slice or reproduce at
the original tested commit.

## Preserve evidence and remove the worktree

Copy any reports you need out of `.mutation-review` along with the tested and
base SHAs, then return to your primary checkout and inspect the worktree before
deleting it:

```sh
git -C .mutation-review status --short
git -C .mutation-review diff
```

Confirm no unique fix or leftover mutation needs rescuing, then remove only the
disposable worktree:

```sh
git worktree remove --force .mutation-review
```

`--force` clears the worktree's untracked reports and build artifacts too; only
ever pass it for this disposable worktree, never your main checkout.

For how scheduled runs accumulate diffs, shard work, and exclude native code,
continue to [CI and releases](ci-and-releases.md#scheduled-mutation-testing).
