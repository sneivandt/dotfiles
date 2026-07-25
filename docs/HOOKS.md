# Git Hooks

The repository installs a pre-commit orchestrator from `hooks\`. It protects the
checkout from obvious secret exposure and Rust quality regressions before CI.

## Files

| File | Purpose |
|---|---|
| `pre-commit` | Entry point installed into the repository's Git hook directory |
| `check-sensitive.sh` | Scans staged content for configured sensitive patterns |
| `sensitive-patterns.ini` | Versioned pattern configuration |
| `sensitive-allowlist.ini` | Versioned allow-list for structurally safe matches |
| `check-rust.sh` | Runs staged-change-aware Rust formatting and checks |
| `check-ci-guards.sh` | Verifies CI publishing and gate invariants |

The installed hook resolves the repository root at runtime and invokes the
source scripts from `hooks\`. This keeps hook logic versioned rather than
duplicated inside `.git\hooks`.

## Default pre-commit flow

```text
check-sensitive.sh
        |
        v
check-rust.sh
```

If either script fails, Git aborts the commit.

The sensitive scan runs first so potential credential exposure is caught before
more expensive Rust checks.

## Full guard mode

Set `DOTFILES_HOOKS_FULL` to `1`, `true`, or `yes` to add CI guard validation:

```bash
DOTFILES_HOOKS_FULL=1 git commit
```

Full mode is useful before changing workflow triggers, publishing guards,
permissions, the `ci-success` dependency list, or artifact behavior.

## Installation and removal

**Git hooks** depends on repository update so it uses current hook sources. The
same visible task label and `git-hooks` selector are used for uninstall.

```bash
dotfiles install --only git-hooks
dotfiles uninstall --dry-run
```

If `hooks\` is absent, repository validation reports a warning rather than
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

Use it only when the hook itself is broken and the change is being used to fix
it. A bypass does not skip CI and should never be used to commit known sensitive
data.

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

Entries currently cover SHA-pinned GitHub Actions (a 40-character hex SHA
frequently contains ten consecutive digits, which the phone and SSN patterns
match by coincidence), the `ci@test.local` CI fixture address, and
`vYYYY.MM.DD.N` release tags, which the IPv4 pattern reads as four
dot-separated numeric groups.

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

Use a fresh scratch repository per run. A failed case leaves its fixture
committed, which makes the next run's identical fixture a no-op diff and
reports a spurious second failure.
