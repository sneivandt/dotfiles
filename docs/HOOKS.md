# Git hooks

The repository installs the `hooks/pre-commit` orchestrator. It catches likely
secrets and Rust check failures before CI.

## Files

| File | Purpose |
|---|---|
| `pre-commit` | Entry point installed into the repository's Git hook directory |
| `check-sensitive.sh` | Scans staged content for configured sensitive patterns |
| `sensitive-patterns.ini` | Versioned pattern configuration |
| `sensitive-allowlist.ini` | Versioned allow-list for structurally safe matches |
| `check-rust.sh` | Runs staged-change-aware Rust formatting and checks |
| `check-ci-guards.sh` | Verifies CI publishing and gate invariants |

At runtime, the installed hook resolves the repository root and invokes scripts
from `hooks/`. The implementation stays under version control instead of being
copied into `.git/hooks`.

## Default pre-commit flow

```text
check-sensitive.sh
        |
        v
check-rust.sh
```

If either check fails, Git aborts the commit.

The sensitive scan runs first so potential credential exposure is caught before
more expensive Rust checks.

## Scan scope

The staged-change scripts use `--diff-filter=d`, which excludes only deletions.
They still scan renames because a renamed file can also add a secret. The diff
includes the old and new paths so Git can pair the rename, while the scanner
reports only added lines.

`check-sensitive.sh` matches each pattern against the added line's own content.
`^` and `$` therefore anchor to the start and end of that line, both for
`sensitive-patterns.ini` and for `sensitive-allowlist.ini`. Reported line
numbers are tracked alongside the content rather than embedded in the scanned
text, so an anchored allow-list entry still matches a construct that begins at
column 0.

## Full guard mode

Set `DOTFILES_HOOKS_FULL` to `1`, `true`, or `yes` to add CI guard validation:

```bash
DOTFILES_HOOKS_FULL=1 git commit
```

Use full mode when changing workflow triggers, publishing guards, permissions,
the `ci-success` dependency list, or artifact behavior.

## Installation and removal

**Git hooks** depends on repository update so it uses current hook sources. The
same visible task label and `git-hooks` selector are used for uninstall.

```bash
dotfiles install --only git-hooks
dotfiles uninstall --dry-run
```

If `hooks/` is absent, repository validation reports a warning rather than
treating the whole configuration as invalid.

## Running checks manually

```bash
sh hooks/check-sensitive.sh
sh hooks/check-rust.sh
sh hooks/check-ci-guards.sh
sh hooks/pre-commit
```

The scripts are POSIX shell scripts and should remain portable. Do not add
Bash-only syntax unless the supported interpreter contract changes.

## Bypassing

Git supports a one-time bypass:

```bash
git commit --no-verify
```

Use this only to fix a broken hook. The bypass does not skip CI, and it must not
be used to commit known sensitive data.

For a recurring false positive, add an allow-list entry instead of bypassing.
A bypass disables the whole scan for that commit; an allow-list entry disables
exactly one known-safe construct and is reviewable.

## Changing sensitive patterns

Treat `sensitive-patterns.ini` and `sensitive-allowlist.ini` as
security-sensitive behavior:

1. Keep broad secret families covered.
2. Make allow-list entries as narrow as possible, and anchor them on
   surrounding syntax rather than on the secret-shaped text alone.
3. Test both a known match and an expected non-match.
4. Avoid including a real credential in a test fixture.
5. Run the hook integration checks.

Allow-list entries redact only the span they match, so the rest of the line is
still scanned. A pinned action reference is ignored, but a credential appearing
later on that same line is still reported.

Current entries cover SHA-pinned GitHub Actions because a 40-character hex SHA
often contains ten consecutive digits that resemble a phone number or SSN. They
also cover the `ci@test.local` CI fixture address.

Reserved documentation domains such as `example.com` are deliberately *not*
allow-listed, because the scanner's own PII coverage is asserted with an
`example.com` address.

## Running the hook test suite

`.github/workflows/scripts/linux/test-git-hooks.sh` creates real commits in the
current repository to prove the hook blocks them. If a detection case ever
fails to block, the test's commit captures whatever else is staged. The script
therefore refuses to run against a checkout with uncommitted changes.

Run it on a clean checkout, or in a scratch repository:

```bash
mkdir -p /tmp/hooktest/hooks && cp -a hooks/. /tmp/hooktest/hooks/
cd /tmp/hooktest && git init -q
ln -sf /tmp/hooktest/hooks/pre-commit .git/hooks/pre-commit
DIR=/path/to/dotfiles sh /path/to/dotfiles/.github/workflows/scripts/linux/test-git-hooks.sh
```

Use a fresh scratch repository for each run. A failed case leaves its fixture
committed. Reusing that repository turns the same fixture into an empty diff and
causes a second, misleading failure.
