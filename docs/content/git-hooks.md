---
title: Git hooks
description: Understand staged checks, sensitive-pattern scanning, Git templates, and how to diagnose a rejected commit.
---

# Git hooks

The dotfiles pre-commit hook checks what you have **staged**, before Git creates
the commit. It catches common mistakes early; it does not replace CI, pre-push
validation, or a review for sensitive data.

There are two independent hook systems:

| System | Purpose | Source |
|---|---|---|
| This repository's pre-commit hook | Check staged changes | [`hooks/`](https://github.com/sneivandt/dotfiles/tree/main/hooks/) |
| Personal Git templates | Protect pushes and clean up merged branches in other repositories too | [`symlinks/config/git/templates/`](https://github.com/sneivandt/dotfiles/tree/main/symlinks/config/git/templates/) |

## Before committing

Review the intended commit, then let Git invoke the installed hook:

```bash
git diff --cached
git commit
```

To check the current index without creating a commit:

```bash
sh hooks/pre-commit
```

These checks do not stage fixes or stash your work. If a file is partially staged,
fixing its working copy is not enough: review and stage the intended fix, then
retry. Do not use `git add .` as a generic cure.

## Default pre-commit flow

The entrypoint runs the sensitive scan first, then Rust/PowerShell checks.
A failure stops the commit. Checks are selected by staged paths:

| Staged change | Check |
|---|---|
| Added text in the staged diff | Sensitive-pattern scan |
| Rust source, including deletion or rename away from `.rs` | Whole-crate `cargo fmt --check` and host Clippy |
| Cargo manifest/lockfile, Cargo config, or toolchain file | The same Rust checks, using the staged compiler pin |
| Present `.ps1`, `.psm1`, or `.psd1` file | PSScriptAnalyzer on those staged files |
| Only TOML data, shell, or workflow files | No Rust compilation solely because of that path; extra guards need full mode |

Clippy uses the `ci` profile, all targets, a locked dependency graph, and warnings
as errors. The hook checks the compiler against the staged
`cli/rust-toolchain.toml`; it does not install or switch toolchains.

If `pwsh` is absent, PowerShell analysis is skipped. If `pwsh` exists but its
PSScriptAnalyzer module is absent or broken, the check fails. **A skipped check
is not coverage.**

## Scan scope

### The index is the input

The sensitive scanner reads the diff from `HEAD` to the index, or from an empty
tree for the first commit. It scans added lines only. An edited rename is still
scanned without treating every unchanged line as newly added.

Rust and CI guards export the **entire index** to an isolated snapshot.
Consequently:

- An unstaged fix cannot hide a staged error.
- An unstaged deletion does not remove the staged file from checks.
- An untracked file cannot satisfy a staged build.
- The working tree is not overwritten or stashed.

Builds reuse the checkout's Cargo target cache, so checks still write build
artifacts. Shell and PowerShell input handling preserves literal filenames,
including spaces and non-ASCII characters; quoted Git display names are not
passed to analyzers.

**The verifier itself comes from the working tree.** The installed entrypoint
finds the checkout with `git rev-parse --show-toplevel` and runs its `hooks/`
helpers. Pattern files and analyzer helpers also come from that checkout.
Review and stage verifier changes separately; an index snapshot does not make
the verifier immutable or trusted.

### What the sensitive scan can detect

Detection rules are case-insensitive extended regular expressions. They match
each added line's text, so `^` and `$` do not anchor to a diagnostic filename or
line number. Findings show file/line locations but redact matched content.

Allowlist matches replace only the safe span before detection. The rest of the
line is still scanned: allowing an Action SHA or a package checksum must not
hide an appended credential. Invalid detection or allowlist expressions fail
the scan, even for an empty diff.

This is not a general secret detector. It does not cover binary data, unchanged
history, or every encoded/unknown credential format. Inspect findings locally;
do not share raw sensitive input or unsanitized logs. See
[Secrets](security-and-trust.md#secrets).

## Full guard mode

Opt into broader, still **path-triggered**, checks:

```bash
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
# Or for the commit itself:
DOTFILES_HOOKS_FULL=1 git commit
```

`true` and `yes` also enable the flag.

| Trigger | Additional coverage |
|---|---|
| Rust/build inputs | Windows GNU-target Clippy when installed, then the full Rust test suite |
| `conf/*.toml` or `symlinks/` | Typed configuration validation and `config_drift` |
| Cargo manifest, lockfile, or deny policy | Wildcard dependency guard and cargo-deny bans/licenses/sources when available |
| Shell or hook paths | ShellCheck when available, on affected shell inputs |
| POSIX wrapper or its test script | Linux wrapper tests |
| Release workflow | Staged publishing invariants: provenance, exact commit handoff, permissions, serialization, and attestation publication |

Deletions and both sides of renames participate in trigger selection. Release
guards read the staged workflow directly; the other guards use the index
snapshot.

You can run `sh hooks/check-ci-guards.sh` directly for its basic targeted checks.
Without full mode it omits config drift, cargo-deny, and wrapper tests.
Neither invocation runs every CI job or checks the full `ci-success` contract.
Use [Testing](testing.md#fast-local-sequence) for the canonical local stages and
[Before pushing](testing.md#before-pushing) for push validation.

## Installation and removal

The `git-hooks` task copies extensionless files from `hooks/` into Git's hook
directory. Currently that means `pre-commit`, not the `.sh` helpers or `.ini`
data. On Unix the installed copy is executable.

Before installing, inspect the destination:

```bash
git config --show-origin --get core.hooksPath
git rev-parse --git-common-dir
```

No output and status 1 from the first command means there is no override.
Dotfiles honors `core.hooksPath`, resolving relative values from the repository
root. Otherwise it uses the common Git directory's `hooks/`, including for
linked worktrees. A global/shared hooks path can affect other repositories.

When you deliberately want to deploy:

```bash
dotfiles update --only git-hooks --dry-run
dotfiles update --only git-hooks
```

**Install can replace a differing destination hook.** Preserve custom hooks
first. A changed copied `pre-commit` needs reinstallation; helper-only changes
take effect directly from the checkout.

To inspect removal, then apply only after review:

```bash
dotfiles remove --only git-hooks --dry-run
dotfiles remove --only git-hooks
```

Removal deletes only hooks matching the current managed source/state. Modified
or mismatched hooks survive. If the configured destination is the source itself,
including through a filesystem alias, install/removal leaves it intact.
Neither operation resets `core.hooksPath`. Missing `hooks/` or Git metadata makes
the task inapplicable.

## Application-wide templates

Git copies template entrypoints when initializing or cloning a repository.
Their shared helpers resolve from the managed template directory at runtime.
These templates are **not** installed by the repository's `git-hooks` task.

### Merged-branch cleanup

`post-merge` and `post-rewrite` after a **rebase** run the shared
`clean-merged-branches` helper. It considers local branches merged into
`origin/HEAD`, while preserving:

- The current branch and the default branch.
- `main`, `master`, `dev`, and `develop`.
- Branches checked out in any worktree.
- Any branch Git refuses to delete safely with `git branch -d`.

Without `origin/HEAD`, cleanup does nothing. An amend does not trigger it.
Existing repositories need a newly introduced template entrypoint, such as
`post-rewrite`, installed explicitly; do not overwrite a repository-specific
hook to do so.

### Push protection

The template pre-push hook blocks branch updates to `main`, `master`, `develop`,
and the destination remote's default branch. It prefers that remote's cached
HEAD, then tries a read-only, noninteractive `ls-remote --symref` lookup.
If discovery fails, the named protections still apply.

Add exact branch names—not globs—with:

```bash
git config --add dotfiles.protectedBranch release/stable
git config --add dotfiles.protectedBranch dev
```

GitHub remotes whose URL owner matches `github.user` (or `github.username`) are
exempt. This is a local configured-name check, not a GitHub permission check.
It is separate from both CI and the repository's pre-push **validation script**.

## Diagnosing a failure

| Symptom | Check next |
|---|---|
| “I fixed it, but it still fails” | Compare `git diff` and `git diff --cached`; stage only the intended fix |
| Formatting failure | Run `cargo fmt` in `cli/`, review the diff, and restage the appropriate hunks |
| Build fails only in the hook | Look for an unstaged or untracked source, dependency, or config needed by the staged code |
| A check was skipped | Read the missing-tool reason and run the required coverage separately |
| Entrypoint edits have no effect | Inspect the real hook destination; reinstall the copied entrypoint deliberately |
| No hook runs | Check `core.hooksPath`, the installed filename, and the Unix executable bit |
| Sensitive finding | Inspect the staged line locally; remove/rotate a real secret or test a narrow safe-span exception |

## Changing the checks

| File | Responsibility |
|---|---|
| [`pre-commit`](https://github.com/sneivandt/dotfiles/blob/main/hooks/pre-commit) | POSIX `sh` orchestration |
| [`check-sensitive.sh`](https://github.com/sneivandt/dotfiles/blob/main/hooks/check-sensitive.sh) | Added-line scanning and redacted diagnostics |
| [`sensitive-patterns.ini`](https://github.com/sneivandt/dotfiles/blob/main/hooks/sensitive-patterns.ini) | Detection expressions |
| [`sensitive-allowlist.ini`](https://github.com/sneivandt/dotfiles/blob/main/hooks/sensitive-allowlist.ini) | Safe-span exceptions |
| [`check-rust.sh`](https://github.com/sneivandt/dotfiles/blob/main/hooks/check-rust.sh) | Staged Rust and PowerShell checks |
| [`check-toolchain.sh`](https://github.com/sneivandt/dotfiles/blob/main/hooks/check-toolchain.sh) | Staged compiler-pin verification |
| [`check-ci-guards.sh`](https://github.com/sneivandt/dotfiles/blob/main/hooks/check-ci-guards.sh) | Targeted configuration, dependency, shell, wrapper, and release guards |

Keep scripts POSIX `sh` compatible and preserve partially staged input,
rename/deletion handling, unusual filenames, and empty-diff behavior.
Installation lives in [`git/hooks.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/git/hooks.rs) and
[`resources/hook.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/git/resources/hook.rs).

### Changing sensitive patterns

Use synthetic fixtures, never live credentials. Test both a detection and the
intended safe case. Anchor an exception to its surrounding syntax, redact the
smallest safe span, and prove that a secret elsewhere on the **same line** is
still detected.

Existing exceptions include Action SHA pins, Cargo/PKGBUILD checksums, fixture
addresses at `test.local`, and GitHub SSH transport syntax. Documentation
addresses at `example.com` are not generally allowlisted.

### Running the hook test suite

Follow [Wrapper and hook tests](testing.md#wrapper-and-hook-tests). The full
integration suite creates commits and can leave a fixture committed when a test
fails to block it. Run it only in the prescribed fresh disposable repository,
**never this checkout**, even when clean. A dirty-tree guard is not isolation.

## Bypassing

`git commit --no-verify` skips the entire pre-commit safeguard, not just one
false positive. It does not skip CI. Reserve it for a deliberately reviewed
recovery from a broken hook, never to commit known sensitive data. Prefer fixing
the check or adding a narrow, tested exception.
