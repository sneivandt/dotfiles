# Test recipes

Use [Testing](TESTING.md#choosing-coverage) to choose coverage first. This page
holds longer commands for targeted fixtures and mutation testing, not an
instruction to run every suite.

Commands start at the repository root. Use the
[pinned Rust toolchain](CONTRIBUTING.md#selecting-the-pinned-toolchain) where
Rust is involved. Install only the tools needed for the chosen recipe.

## Wrapper and hook tests

### Isolated regressions

These use fixture tools rather than bootstrapping or installing the real CLI:

```sh
sh .github/workflows/scripts/linux/test-hook-inputs.sh test_staged_ci_guards
sh .github/workflows/scripts/linux/test-hook-inputs.sh test_ci_change_classification
sh .github/workflows/scripts/linux/test-hook-inputs.sh test_staged_ci_guard_deletions
sh .github/workflows/scripts/linux/test-shell-wrapper.sh \
  test_wrapper_preserves_runtime_context test_wrapper_uses_cargo_artifact
```

With no selector, `test-hook-inputs.sh` runs all input regressions. Its
`test_build_version_ref_triggers` case compiles the std-only `cli/build.rs`
directly to test Git-ref invalidation of build metadata. It needs the pinned
compiler; without it, the case skips.

On Windows, dot-source the wrapper suite to load functions without running the
full integration suite:

```powershell
. .\.github\workflows\scripts\windows\Test-ShellWrapper.ps1
Test-IsolatedWrapperPath
Test-CargoArtifactPath
```

Full wrapper suites may build/copy binaries or use `BINARY_PATH`. Read their
setup before running all cases.

### Checking your real index

These commands inspect staged content and do not create commits:

```sh
sh hooks/check-sensitive.sh
sh hooks/check-rust.sh
sh hooks/check-ci-guards.sh
DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
```

The Rust and CI guards export the index and reuse Cargo caches without stashing,
overwriting, or staging your working files. Helper code and sensitive-pattern
rules are read from the checkout. Full mode is still staged-change-aware,
not a substitute for committed-range validation. See [Hooks](HOOKS.md).

### Full commit-hook suite

**Never run `test-git-hooks.sh` in your developer checkout.** It creates real
commits in its current repository. Its dirty-tree guard helps prevent accidents,
but a clean tree is not a safe fixture; do not use its force override.

This setup creates a fresh scratch repository, copies only the hooks, and keeps
the real checkout as the source of the test script:

```sh
(
  set -eu
  source_root="$PWD"
  fixture=$(mktemp -d)
  printf 'Hook fixture: %s\n' "$fixture"
  mkdir "$fixture/hooks"
  cp -R hooks/. "$fixture/hooks/"
  cd "$fixture"
  unset GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_INDEX_FILE
  unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES
  unset GIT_CONFIG GIT_CONFIG_PARAMETERS
  export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_COUNT=0
  git init -q --template=
  git config core.hooksPath .git/hooks
  git config user.name "Hook Test"
  git config user.email "ci@test.local"
  git add hooks
  git commit -qm baseline
  cp hooks/pre-commit .git/hooks/pre-commit
  chmod +x .git/hooks/pre-commit
  DIR="$source_root" sh "$source_root/.github/workflows/scripts/linux/test-git-hooks.sh"
)
```

Inspect and remove only the printed scratch directory afterward. Do not reuse
a failed fixture: committed test data can turn a later check into an empty diff.
Broader installation/application suites also need a disposable home or runner,
not just a separate Git directory.

## Cleanup retention and Git templates

```sh
python3 -B .github/workflows/scripts/linux/test-maintenance.py
```

This creates isolated repositories and runs cleanup `find` commands against
temporary directories, without starting systemd services. It checks merge/rebase
cleanup, protected branches, worktrees, and file/link/directory retention.

The cleanup services use `-mtime +7` for `~/tmp` and `-mtime +30` for
`~/screenshots`. Because `find` counts completed 24-hour periods, removal starts
at eight and thirty-one days. Symlinks are removed without following targets.
Directories must be empty and old enough; removing their contents can refresh
their modification time and delay deletion.

## Desktop shell

QML logic/layout tests require Qt 6.8 or newer Quick Test:

```sh
sh .github/workflows/scripts/linux/check.sh --strict qml
```

The runner uses inert Quickshell mocks and does not start live services or load
installed desktop configuration. Native rendering and focus still need manual
testing in the relevant desktop session.

Python helpers and stock-cache concurrency use isolated fixtures:

```sh
python3 -B -m unittest discover \
  -s symlinks/config/quickshell/tests/python -p 'test_*.py'
python3 -B -m unittest discover \
  -s symlinks/config/hypr/scripts/tests -p 'test_*.py'
sh .github/workflows/scripts/linux/test-stocks.sh
```

They mock desktop actions and market requests. On Windows use `python -B`;
Linux-only cases skip. A successful fixture does not prove live compositor,
network, or screen-lock behavior.

## Application configuration

These use isolated homes and mock commands:

```sh
python3 -B .github/workflows/scripts/linux/test-shell-config.py
python3 -B -m unittest discover -s symlinks/vim/tests -p 'test_*.py'
pwsh -NoProfile -File symlinks/config/powershell/tests/Test-Prompt.ps1
```

The PowerShell fixture covers first-start command discovery, custom Cargo paths,
literal profile-fragment directories, and PATH deduplication on reload.
Install each suite's required native tools; report missing-tool skips.

## CI selection fixtures

```sh
python3 -B .github/workflows/scripts/linux/test-ci-changes.py
```

This tests classification, aggregate gating, renames/deletions, unusual paths,
invalid ranges, and published-release baselines. It uses temporary Git
repositories and a mock `gh`: fixture commits/tags are local and no releases
are published.

To run only the pure classification/gate cases:

```sh
python3 -B .github/workflows/scripts/linux/test-ci-changes.py \
  ClassificationTests GateTests
```

Use the local `ci` stage as well when workflow contracts or syntax changed.
See [CI and releases](CI.md).

## Focused mutation testing

`cargo-mutants` deliberately changes source. Install it separately and run it
in a disposable checkout of the committed patch. The following Linux recipe
uses `origin/main` as the comparison base; replace it with the destination's
previous commit when needed.

```sh
(
  source_root="$PWD"
  mutation_base=$(git rev-parse --verify 'origin/main^{commit}') || exit 1
  mutation_head=$(git rev-parse --verify 'HEAD^{commit}') || exit 1
  mutation_root=$(mktemp -d) || exit 1
  printf 'Mutation fixture: %s\n' "$mutation_root"
  git clone --quiet --template= --shared --no-checkout \
    "$source_root" "$mutation_root/source" || exit 1
  git -C "$mutation_root/source" -c core.hooksPath=/dev/null \
    checkout --quiet --detach "$mutation_head" || exit 1
  cd "$mutation_root/source/cli" || exit 1
  git diff --relative=cli --unified=0 \
    "$mutation_base" "$mutation_head" -- src > mutants.diff || exit 1
  if [ ! -s mutants.diff ]; then
    echo "No changed Rust source; mutation testing did not run."
    exit 0
  fi
  export XDG_STATE_HOME="$mutation_root/state" DOTFILES_LOG_DIR="$mutation_root/logs"
  cargo mutants --in-place --no-shuffle --in-diff mutants.diff \
    --exclude-re '^src/infra/elevation\.rs:.*\b(run_elevated_child|wait_if_elevated)\b' \
    --exclude-re '^src/domains/dotfiles/resources/path_entry\.rs:.*\b(decode_reg_string|append_user_path|user_path_contains)\b' \
    --exclude-re '^src/domains/ai/apm/cowork\.rs:.*\bis_desired_skill_directory\b' \
    --exclude-re '^src/domains/system/resources/registry\.rs:.*\bnative::' \
    --exclude-re '^src/domains/ai/resources/agent_settings\.rs:[0-9]+:[0-9]+: replace match guard metadata\.is_symlink\(\) with true in <impl Resource for AgentSettingResource>::apply$' \
    --cargo-arg=--profile=ci --cargo-arg=--locked
  mutation_status=$?
  echo "Results: $mutation_root/source/cli/mutants.out"
  exit "$mutation_status"
)
```

Inspect results before removing the printed temporary directory. To narrow a
costly run, add `--file src/path/to/module.rs` or `--re 'function_name'` and
record that reduced scope.

The exclusions mirror Linux CI: Windows-only elevation, registry, PATH, and
Cowork directory-identity code cannot receive native Linux coverage. There is
also one equivalent agent-settings mutation that canonicalizes a regular file
without changing its write destination. Keep exclusions aligned with
[`ci.yml`](../.github/workflows/ci.yml), not a growing local ignore list.

Linux CI divides the applicable changed-code mutants into eight shards; omit
`--shard` locally to cover the selected set. Investigate every `MISSED` mutant.
It may reveal a missing assertion or equivalent behavior, not necessarily a
production bug. Native Windows tests remain necessary, and Windows mutation
coverage is not provided.
