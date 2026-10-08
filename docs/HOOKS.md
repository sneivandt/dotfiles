# Git hooks

The repository's pre-commit hook is a local safeguard for the **staged commit**,
not a substitute for CI or a guarantee that a repository contains no secrets.
This guide explains what it runs, which version of a file it sees, and how to
diagnose a failure without losing partially staged work.

## Files

| Source | Role |
|---|---|
| [`pre-commit`](../hooks/pre-commit) | Installed entrypoint; delegates to checkout helpers |
| [`check-sensitive.sh`](../hooks/check-sensitive.sh) | Scans added staged lines for configured sensitive patterns |
| [`sensitive-patterns.ini`](../hooks/sensitive-patterns.ini) | Case-insensitive extended-regex detection rules |
| [`sensitive-allowlist.ini`](../hooks/sensitive-allowlist.ini) | Safe spans to redact before matching, not whole-line exemptions |
| [`check-rust.sh`](../hooks/check-rust.sh) | Staged-change-aware Rust and PowerShell checks |
| [`check-ci-guards.sh`](../hooks/check-ci-guards.sh) | Targeted configuration, dependency, shell, wrapper and release guards |

These are POSIX `sh` scripts; keep that interpreter contract when editing.
Application-wide Git templates under
[`symlinks/config/git/templates/`](../symlinks/config/git/templates) are a
separate mechanism, not extra entrypoints selected from this `hooks/` directory.

## Default pre-commit flow

```text
check-sensitive.sh -> check-rust.sh -> commit proceeds
        any failure ----------------> commit aborts
```

The sensitive scan always runs first. The remaining work depends on staged paths:

| Staged change | Default behavior |
|---|---|
| Added/modified/renamed text | Sensitive-pattern scan of added lines |
| `.rs` change, including deletion or rename away from `.rs` | Whole-crate formatting and host Clippy (`ci` profile, all targets, warnings denied) |
| Present `.ps1`/`.psm1`/`.psd1` change | PSScriptAnalyzer on those staged files, if `pwsh` exists |
| Only a Cargo manifest/lockfile, TOML data, shell script or workflow | No Rust compilation solely because of that path; use full guards/local checks as applicable |

Missing `pwsh` skips PowerShell analysis. If `pwsh` exists but PSScriptAnalyzer
is missing or broken, the check fails. The hook does not install tools for you;
Rust-triggered checks require the configured Cargo toolchain.

## Scan scope

### The index is the input

The sensitive scan compares the index to `HEAD` (or an empty tree for the first
commit). Only added lines are scanned; deleted content and unchanged history
are not. Renames are included with both old and new paths so an edited rename
cannot evade scanning, without treating every unchanged line as an addition.
File/line locations are printed, but matched contents are reported as
`<redacted>`.

Rust and CI guards export the **entire index** into an isolated snapshot before
running their checks. An unstaged fix cannot hide a staged error, an unstaged
deletion cannot hide a staged file, and untracked files cannot satisfy the
build. These guards do not stash or overwrite the working tree. They reuse the
checkout's Cargo target cache, so a staged build still writes build artifacts.

**The check implementation is different from the checked input.** The installed
entrypoint resolves `git rev-parse --show-toplevel` and runs helpers from that
checkout's `hooks/`. Helpers and sensitive-pattern/allowlist files are therefore
read from the working tree. If you are changing the checks themselves, review
and stage those changes too; exported input does not mean an immutable or
trusted verifier.

### Regex and allowlist behavior

Patterns match each added line's own text. `^` and `$` anchor to that content,
not to a prefixed line number. Allowlist matches are replaced by a safe marker
before detection; the rest of the line is still scanned. For example, a pinned
GitHub Action SHA can be allowed without allowing a credential later on the
same line.

This is pattern matching, not a general secret detector. Binary content,
unchanged historical secrets and unrecognized/encoded values are not covered.
Do not upload raw scanner input, private overlays or unsanitized logs to explain
a failure. See [Security](SECURITY.md#secrets).

## Full guard mode

`DOTFILES_HOOKS_FULL=1`, `true` or `yes` enables the extra checks:

```bash
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

It also applies when Git invokes the hook, for example with
`DOTFILES_HOOKS_FULL=1 git commit`. Checks remain **path-triggered**:

- Rust changes add Windows-target Clippy when that target is installed, then
  the full Rust test suite. A missing Windows target is reported as skipped.
- `conf/*.toml` or `symlinks/` changes trigger typed configuration validation
  and `config_drift`.
- Cargo manifest, lockfile or deny-policy changes trigger the wildcard-version
  guard and, when available, cargo-deny bans/licenses/sources checks.
- Shell/hook changes trigger ShellCheck when available.
- POSIX wrapper or its test-script changes trigger Linux wrapper tests.
- Release-workflow changes trigger staged publishing guards for CI provenance,
  exact commit handoff, permissions, serialization and attestation publication.

Configuration/shell/dependency/wrapper checks operate on the exported index;
release guards read the staged workflow directly. Classification includes
deletions and both sides of renames, so removing a required file still requests
validation.

Running `sh hooks/check-ci-guards.sh` directly enables its basic targeted checks
without the environment flag; expensive additions still require full mode.
It does **not** run every CI job or the aggregate `ci-success` contract checker.
For aggregate job membership/scheduling, run the `ci` stage documented in
[Testing](TESTING.md#fast-local-sequence).

## Installation and removal

The [`git-hooks` task](../cli/src/domains/git/hooks.rs) discovers extensionless
files in `hooks/`. It installs `pre-commit`, not the `.sh` helpers
or `.ini` data, as a **copy** using the
[`hook resource`](../cli/src/domains/git/resources/hook.rs). On Unix it makes the
installed file executable.

Destination selection honors `core.hooksPath` (relative to the repository root
when relative), otherwise Git's common-directory `hooks/`. Linked worktrees
have a `.git` file, not a private `.git/hooks/` directory; the common-directory
handling matters. A shared/global hooks path can affect more than this checkout,
so inspect it before installing:

```bash
git config --show-origin --get core.hooksPath
git rev-parse --git-common-dir
```

No output/status 1 from the first command means no configured override.
When you deliberately want to deploy the hook:

```bash
dotfiles update --root . --no-repo-update --only git-hooks --dry-run
dotfiles update --root . --no-repo-update --only git-hooks
dotfiles remove --root . --only git-hooks --dry-run
```

These are installation commands, not tests. Install can replace a differing
hook at the destination; inspect custom hooks first. A changed copied
`pre-commit` requires reinstalling that entrypoint. Helper-only edits are picked
up immediately from the checkout.

In the full update graph the catalog orders hook installation after repository
update; it is an ordering-only edge, not failure propagation. Missing `hooks/`
or Git metadata makes the task inapplicable; repository validation warns about
missing `hooks/`.

Uninstall removes a hook only when its current state matches the managed
source; modified/mismatched hooks are preserved. If `core.hooksPath` resolves
directly to source hooks, including through a filesystem alias, install/removal
leaves those sources intact. Neither command resets your `core.hooksPath`.

## Running checks manually

Use [Wrapper and hook tests](TESTING.md#wrapper-and-hook-tests) for commands,
isolated staged-input regressions, and the full-suite safety procedure.
Direct helper invocations test your current index without creating a commit.

### Diagnosing a failure

| Symptom | Check next |
|---|---|
| “I fixed it, but the hook still fails” | Compare `git diff` with `git diff --cached`; stage the intended fix, not unrelated hunks |
| Formatting failure | Run `cargo fmt` from `cli/`, inspect the resulting diff, then restage only intended changes |
| Clippy/build failure only in the hook | Look for an unstaged dependency/config/source change needed by the staged code |
| Analyzer or cross-target check skipped | Read tool availability; a skip is not coverage |
| Old entrypoint behavior after editing `pre-commit` | Inspect the actual hooks destination and deliberately reinstall the copy |
| Hook never runs | Check `core.hooksPath`, the installed filename and Unix executable bit |
| Sensitive match | Inspect the staged line locally; remove/rotate real secrets or narrowly cover a demonstrably safe construct |

Avoid `git add .` as a generic repair for partial-staging problems.

## Bypassing

Git's `git commit --no-verify` skips the entire pre-commit safeguard, not just one
false positive. It does not skip CI. Reserve it for a deliberately reviewed
recovery from a broken hook, never for committing known sensitive data.
Prefer fixing the hook or a narrow, tested allowlist entry for recurring false
positives.

## Changing sensitive patterns

1. Use synthetic fixtures, never live credentials or private data.
2. Cover both detection and the intended safe case.
3. Anchor exceptions to surrounding syntax and redact the smallest safe span.
4. Test that an unrelated secret on the **same line** is still detected.
5. Preserve rename, partial-staging, anchored-pattern and redacted-output
   regressions.

Current exceptions include SHA-pinned GitHub Actions and the `ci@test.local`
fixture identity. Documentation domains such as `example.com` are not generally
allowlisted; they are also used to exercise PII detection. Do not weaken a broad
secret family just to silence one example.

## Running the hook test suite

The full suite creates commits and can leave a fixture committed when a case
fails to block. Run it only in the fresh disposable repository described in
[Testing](TESTING.md#wrapper-and-hook-tests), never in the developer checkout,
even if clean. Its dirty-tree guard is a backup, not an isolation mechanism.
