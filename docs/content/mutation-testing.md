---
title: Mutation testing
description: Check whether tests detect incorrect Rust behavior using a committed source range and a disposable worktree.
---

# Mutation testing

Mutation testing temporarily changes code to check whether tests notice a
wrong branch, lost error, or incorrect preview. Run focused mutation tests for
Rust behavior changes, especially failure handling, scheduling, and partial success.
Use [Testing](testing.md#choosing-coverage) to select the other checks for your patch.

Run `cargo-mutants` in a disposable checkout, never the shared working copy.
For a `MISSED` result, add an assertion that passes on the original code and
fails with the mutation, or establish that the mutant is equivalent. Selected
shards with survivors, timeouts, or tooling failures block CI. Ordinary tests
and the local checker's `--all` do not run mutation testing.

## Prepare an isolated source snapshot

Install `cargo-mutants` only if you intend to run this check. Use the pinned
Rust toolchain and a native filesystem for the platform under test; Windows
and Linux mutations cover different runtime contracts.

Commit the source and tests you want to check first. From the primary checkout,
run the [pre-push plan](testing.md#implementation-and-mixed-patches) and record
the left SHA of its `Committed range: BASE..HEAD` line. The default planner verifies
the upstream CI baseline and can recover to a successful ancestor. If you
provided `--base`, you are responsible for that baseline's verification.

In the commands below, replace `ORIGINAL_BASE_SHA` with that full SHA.
Keep it with the mutation report and reuse it after test-only fixes. Do not
replace it with a failed push's tip or a newly advanced upstream branch.

From the primary repository root, create a detached worktree in the unused
`.mutation-review` directory:

```sh
git worktree add --detach .mutation-review HEAD
```

Only committed files are copied. Uncommitted and untracked fixes in the primary
checkout are not part of this test. Never stage the disposable directory or its
reports as part of your patch.

## Select and run the mutations

Enter `.mutation-review/cli` in a POSIX shell, or
`.mutation-review\cli` in PowerShell. Generate a source-only diff using the
recorded baseline:

```sh
git diff --relative=cli --no-renames --unified=0 --output=mutants.diff ORIGINAL_BASE_SHA HEAD -- src
cargo mutants --in-place --in-diff mutants.diff --cargo-arg=--profile=ci --cargo-arg=--locked
```

These two commands also work in PowerShell. Git writes the diff itself so
shell redirection cannot change its encoding. Check that the diff contains the
intended source changes before running the second command. A test-only recovery
should still include the original source change. An empty diff supplies no
changed-code coverage.

`--in-place` confines source mutations to this disposable worktree. To test a
specific module rather than the committed diff, substitute:

```sh
cargo mutants --in-place --file src/app/filter.rs --cargo-arg=--profile=ci --cargo-arg=--locked
```

Use `--re` to replay a specific mutant by its report description. Inspect
`mutants.out` for survivors, timeouts, tool errors, and which tests actually ran.
After adding and committing assertions in the primary checkout, preserve the
old report, remove the disposable worktree, and repeat this procedure at the
new commit with the same `ORIGINAL_BASE_SHA`.

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

CI handles changed-code selection, shards, and native-code exclusions; see
[Mutation coverage](ci-and-releases.md#platform-coverage). Linux checks do not
provide native Windows mutation coverage.
