# Git Hooks

The repository installs a pre-commit orchestrator from `hooks\`. It protects the
checkout from obvious secret exposure and Rust quality regressions before CI.

## Files

| File | Purpose |
|---|---|
| `pre-commit` | Entry point installed into the repository's Git hook directory |
| `check-sensitive.sh` | Scans staged content for configured sensitive patterns |
| `sensitive-patterns.ini` | Versioned pattern and allow-list configuration |
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

**Install Git hooks** runs in the Sync phase after repository update so it uses
current hook sources. **Remove Git hooks** is part of uninstall.

```bash
dotfiles install --only "Git hooks"
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

## Changing sensitive patterns

Treat `sensitive-patterns.ini` as security-sensitive behavior:

1. Keep broad secret families covered.
2. Make allow-list entries as narrow as possible.
3. Test both a known match and an expected non-match.
4. Avoid including a real credential in a test fixture.
5. Run the hook integration checks.
