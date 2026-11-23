---
title: Mutation testing
description: Check whether tests detect incorrect Rust behavior using a committed source range and a disposable worktree.
---

# Mutation testing

Mutation testing temporarily changes code to check whether tests notice a
wrong branch, lost error, or incorrect preview. Select local mutation checks
when they add evidence for safety or failure behavior, or when investigating a
survivor. They are not a required local gate for every Rust edit. Use the
[local decision table](testing.md#focused-mutation-testing) to choose when
mutation testing adds evidence.

Run `cargo-mutants` in a disposable checkout, never the shared working copy.
For a `MISSED` result, add an assertion that passes on the original code and
fails with the mutation, or establish that the mutant is equivalent. Survivors,
timeouts, and tooling failures fail the mutation check. The
[nightly/manual workflow](ci-and-releases.md#scheduled-mutation-testing)
does not block ordinary CI or publishing. Ordinary tests and the local
checker's `--all` do not run mutation testing.

## Prepare an isolated source snapshot

Install cargo-mutants 27.1.0 to match the hosted workflow. Use the pinned
Rust toolchain and a native filesystem for the platform under test; Windows
and Linux mutations cover different runtime contracts.

For a hosted failure, open the **Mutation: Results** summary. Record the base
and tested commit, and use the listed focused command to replay a survivor or
timeout. To reproduce the original location, start at the tested commit. After
committing a fix, use the new commit and the same base to recheck the full
selection, even if the fix only changes tests.

For a change that has not run in CI, commit the source and tests, then use the
left SHA of the [pre-push plan](testing.md#implementation-and-mixed-patches)'s
committed range as the base. If you provided a custom base to the planner,
verify that it covers the changes you intend to test.

Create a disposable worktree from the repository root. Use the tested SHA in
place of HEAD when reproducing a hosted failure.

```sh
git worktree add --detach .mutation-review HEAD
```

Only committed files are copied. Keep this checkout and its reports out of
your patch.

## Select and run the mutations

Run the following commands from the root of the disposable worktree. Replace
ORIGINAL_BASE_SHA with the recorded full SHA. Use `--all-source` instead of
`--base ORIGINAL_BASE_SHA` when the report says the run used all source without
a passing baseline. The selector uses the same whole-file and shared-input
rules as the hosted workflow.

```sh
python3 -B .github/workflows/scripts/linux/select-mutations.py --base ORIGINAL_BASE_SHA
```

On Linux, paste the focused command from the report or use `run linux` to run
the ordinary mutants in the selection. Use `run liveness`, `run context`, or
`run process` for the suites with bounded test targets. For instance:

```sh
python3 -B .github/workflows/scripts/linux/run-mutations.py run process
```

On Windows, use `python` in the selection command and run the native suite
on a Windows filesystem.

```powershell
python -B .github/workflows/scripts/linux/run-mutations.py run windows
```

The replay commands mutate the disposable worktree in place. Inspect
`cli/mutants.out` for survivors, timeouts, and tool errors. Save any report
you need before running another suite, because cargo-mutants rotates this
directory. If a focused replay reports that the mutation description changed,
rerun the original suite or reproduce it at the tested commit.

## Preserve evidence and remove the worktree

Copy any reports you need to keep out of `.mutation-review`, along with the
tested commit and original baseline SHA. Return to the primary repository root
and inspect the disposable checkout:

```sh
git -C .mutation-review status --short
git -C .mutation-review diff
```

Make sure no unique fix or interrupted source mutation needs recovering.
After saving that work and the reports, remove only this disposable worktree:

```sh
git worktree remove --force .mutation-review
```

The force option also deletes its untracked and ignored reports and build
artifacts. It is appropriate here only because this worktree was created for
the check; never point it at the primary checkout.

The separate workflow handles accumulated selection, shards, and native-code
exclusions; see [Scheduled mutations](ci-and-releases.md#scheduled-mutation-testing). Linux checks do not
provide native Windows mutation coverage.
