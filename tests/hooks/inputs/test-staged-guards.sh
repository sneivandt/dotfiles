#!/bin/sh
# shellcheck shell=sh

test_fixture_git_context()
{(
  log_stage "Checking fixture isolation from an inherited Git repository and index"
  fixture="$DIR/.ci-git-context-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir "$fixture/caller" "$fixture/child"
  cd "$fixture/caller" || exit 1
  init_fixture_repository
  printf 'caller\n' > marker
  git add marker
  caller_tree=$(git write-tree)
  (
    cd "$fixture/child" || exit 1
    init_fixture_repository
    printf 'child\n' > marker
    git add marker
    [ "$(git show :marker)" = child ] || log_error "Child fixture used the caller index"
  )
  [ "$(git write-tree)" = "$caller_tree" ] || log_error "Child fixture changed the caller index"
)}

test_staged_ci_guards()
{(
  log_stage "Checking CI guards use staged shell, manifest, and config content"
  fixture="$DIR/.ci-guard-inputs-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
  mkdir -p "$fixture/mock-bin" "$fixture/repo [literal]/hooks" "$fixture/repo [literal]/cli" "$fixture/repo [literal]/conf"
  cat > "$fixture/mock-bin/shellcheck" <<'EOF'
#!/bin/sh
set -eu
for file do
  case "$file" in --*) continue ;; esac
  [ "$(cat "$file")" = valid-staged ]
done
printf 'shellcheck\n' >> "$HOOK_INPUT_LOG"
EOF
  cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = run ]
[ -n "${DOTFILES_VERSION:-}" ]
case "${CARGO_TARGET_DIR:-}" in
  */cli/target/hooks) ;;
  *) exit 1 ;;
esac
while [ "$#" -gt 0 ]; do
  if [ "$1" = --root ]; then
    shift
    case "$1" in
      */.git/*) exit 1 ;;
    esac
    [ "$(cat "$1/conf/fixture.toml")" = staged-config ]
    [ ! -e "$1/conf/untracked.toml" ]
    printf 'config\n' >> "$HOOK_INPUT_LOG"
    exit 0
  fi
  shift
done
exit 1
EOF
  chmod +x "$fixture/mock-bin/"*
  unset CARGO_TARGET_DIR DOTFILES_VERSION
  PATH="$fixture/mock-bin:$PATH"
  HOOK_INPUT_LOG="$fixture/checks.log"
  export PATH HOOK_INPUT_LOG
  cd "$fixture/repo [literal]" || exit 1
  init_fixture_repository
  printf 'valid-staged\n' > 'hooks/script with spaces.sh'
  printf '[dependencies]\nexample = "1"\n' > cli/Cargo.toml
  printf 'staged-config\n' > conf/fixture.toml
  git add hooks cli conf
  printf 'invalid-unstaged\n' > 'hooks/script with spaces.sh'
  printf 'unstaged-config\n' > conf/fixture.toml
  printf 'untracked-config\n' > conf/untracked.toml
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = "$(printf 'config\nshellcheck')" ] ||
    log_error "Staged script and configuration were not both checked"

  printf 'invalid-staged\n' > 'hooks/script with spaces.sh'
  git add hooks
  printf 'valid-staged\n' > 'hooks/script with spaces.sh'
  if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "CI guards accepted invalid staged shell content"
  fi
  grep -q 'ShellCheck reported issues' "$fixture/output" || log_error "Unexpected staged shell failure"
  git add hooks
  rm 'hooks/script with spaces.sh'
  printf 'staged-config\n' > conf/fixture.toml
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = "$(printf 'config\nshellcheck')" ] ||
    log_error "Unstaged deletion hid a staged script or configuration check"

  printf 'invalid-staged\n' > conf/fixture.toml
  git add conf/fixture.toml
  printf 'staged-config\n' > conf/fixture.toml
  if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "CI guards accepted invalid staged config hidden by an unstaged fix"
  fi
  grep -q 'configuration validation failed' "$fixture/output" || log_error "Unexpected staged config failure"
  git add conf/fixture.toml

  printf '[dependencies]\nexample = "*"\n' > cli/Cargo.toml
  git add cli
  printf '[dependencies]\nexample = "1"\n' > cli/Cargo.toml
  if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "CI guards accepted a staged wildcard dependency hidden by an unstaged fix"
  fi
  grep -q 'wildcard dependency' "$fixture/output" || log_error "Unexpected staged manifest failure"
)}

test_staged_quoted_shell()
{(
  log_stage "Checking shell hook arguments with quoted Git filenames"
  fixture="$DIR/.ci-quoted-shell-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir -p "$fixture/repo/hooks" "$fixture/repo/cli/src/app/validation/scripts" "$fixture/mock-bin"
  cp "$DIR/hooks/check-ci-guards.sh" "$fixture/repo/hooks/check-ci-guards.sh"
  # Isolate the hook's selection and forwarding from the analyzer itself.
  cat > "$fixture/repo/cli/src/app/validation/scripts/shellcheck.sh" <<'EOF'
#!/bin/sh
set -eu
[ "$#" -eq 1 ]
[ "${1##*/}" = "$HOOK_EXPECTED_SHELL_FILE" ]
printf 'shellcheck\n' >> "$HOOK_INPUT_LOG"
[ "$(cat "$1")" = valid-staged ]
EOF
  printf '#!/bin/sh\nexit 0\n' > "$fixture/mock-bin/shellcheck"
  chmod +x "$fixture/mock-bin/shellcheck"
  PATH="$fixture/mock-bin:$PATH"
  HOOK_INPUT_LOG="$fixture/checks.log"
  export PATH HOOK_INPUT_LOG HOOK_EXPECTED_SHELL_FILE
  cd "$fixture/repo" || exit 1
  init_fixture_repository
  for file in 'script with spaces.sh' "$(printf 'caf\303\251.sh')" \
    "$(printf 'tab\tname.sh')" "$(printf 'line\nbreak.sh')" 'quote"name.sh' \
    '-option.sh' 'glob[name].sh'; do
    # Windows prohibits control characters and double quotes in filenames.
    if command -v cygpath >/dev/null 2>&1; then
      case "$file" in *'"'*|*"$(printf '\t')"*|*'
'*) continue ;; esac
    fi
    HOOK_EXPECTED_SHELL_FILE=$file
    printf 'valid-staged\n' > "$file"
    git add -- "$file"
    printf 'invalid-unstaged\n' > "$file"
    : > "$HOOK_INPUT_LOG"
    sh hooks/check-ci-guards.sh
    [ "$(cat "$HOOK_INPUT_LOG")" = shellcheck ] || log_error "Quoted shell input was skipped"
    printf 'invalid-staged\n' > "$file"
    git add -- "$file"
    printf 'valid-staged\n' > "$file"
    if sh hooks/check-ci-guards.sh > "$fixture/output" 2>&1; then
      log_error "Quoted shell filename hid invalid staged content"
    fi
    grep -q 'ShellCheck reported issues' "$fixture/output" || log_error "Unexpected shell failure"
    git add -- "$file"
    git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null commit -qm fixture
    renamed="renamed $file"
    git mv -- "$file" "$renamed"
    HOOK_EXPECTED_SHELL_FILE=$renamed
    : > "$HOOK_INPUT_LOG"
    sh hooks/check-ci-guards.sh
    [ "$(cat "$HOOK_INPUT_LOG")" = shellcheck ] || log_error "Renamed shell input was skipped"
    git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null commit -qm rename
    git rm -q -- "$renamed"
    : > "$HOOK_INPUT_LOG"
    sh hooks/check-ci-guards.sh
    [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Deleted shell input was analyzed"
    git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null commit -qm deletion
    sh hooks/check-ci-guards.sh
    [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Empty staged diff invoked ShellCheck"
  done
)}

test_staged_quoted_powershell()
{(
  log_stage "Checking PowerShell hook arguments with quoted Git filenames"
  fixture="$DIR/.ci-quoted-powershell-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir "$fixture/repo" "$fixture/mock-bin"
  cat > "$fixture/mock-bin/pwsh" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = -NoProfile ] && [ "$2" = -File ]
shift 3
[ "$#" -eq 1 ]
[ "$1" = "./$HOOK_EXPECTED_FILE" ]
printf 'powershell\n' >> "$HOOK_INPUT_LOG"
[ "$(cat "$1")" = valid-staged ]
EOF
  chmod +x "$fixture/mock-bin/pwsh"
  PATH="$fixture/mock-bin:$PATH"
  HOOK_INPUT_LOG="$fixture/checks.log"
  export PATH HOOK_INPUT_LOG HOOK_EXPECTED_FILE
  cd "$fixture/repo" || exit 1
  init_fixture_repository
  for file in 'script with spaces.ps1' "$(printf 'tab\tname.psm1')" \
    "$(printf 'line\nbreak.psd1')" 'quote"name.ps1' '-option.ps1' 'glob[name].ps1'; do
    # Windows prohibits control characters and double quotes in filenames.
    if command -v cygpath >/dev/null 2>&1; then
      case "$file" in *'"'*|*"$(printf '\t')"*|*'
'*) continue ;; esac
    fi
    HOOK_EXPECTED_FILE=$file
    printf 'valid-staged\n' > "$file"
    git add -- "$file"
    printf 'invalid-unstaged\n' > "$file"
    : > "$HOOK_INPUT_LOG"
    sh "$DIR/hooks/check-rust.sh"
    [ "$(cat "$HOOK_INPUT_LOG")" = powershell ] || log_error "PowerShell input was skipped"
    printf 'invalid-staged\n' > "$file"
    git add -- "$file"
    printf 'valid-staged\n' > "$file"
    if sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
      log_error "Quoted PowerShell filename hid invalid staged content"
    fi
    grep -q 'PSScriptAnalyzer reported issues' "$fixture/output" || log_error "Unexpected PowerShell failure"
    git add -- "$file"
    git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null commit -qm fixture
    renamed="renamed $file"
    git mv -- "$file" "$renamed"
    HOOK_EXPECTED_FILE=$renamed
    : > "$HOOK_INPUT_LOG"
    sh "$DIR/hooks/check-rust.sh"
    [ "$(cat "$HOOK_INPUT_LOG")" = powershell ] || log_error "Renamed PowerShell input was skipped"
    git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null commit -qm rename
    git rm -q -- "$renamed"
    : > "$HOOK_INPUT_LOG"
    sh "$DIR/hooks/check-rust.sh"
    [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Deleted PowerShell input was analyzed"
    git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null commit -qm deletion
    sh "$DIR/hooks/check-rust.sh"
    [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Empty staged diff invoked PSScriptAnalyzer"
  done
  # Ensure argv construction also preserves multiple staged inputs in one batch.
  cat > "$fixture/mock-bin/pwsh" <<'EOF'
#!/bin/sh
set -eu
shift 3
[ "$#" -eq 2 ]
[ "$1" = ./first.ps1 ] && [ "$2" = './second file.psd1' ]
for file do
  [ "$(cat "$file")" = valid-staged ]
done
printf 'powershell-batch\n' >> "$HOOK_INPUT_LOG"
EOF
  printf 'valid-staged\n' > first.ps1
  printf 'valid-staged\n' > 'second file.psd1'
  git add -- first.ps1 'second file.psd1'
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-rust.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = powershell-batch ] || log_error "PowerShell batch was skipped"
  printf 'invalid-staged\n' > 'second file.psd1'
  git add -- 'second file.psd1'
  if sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
    log_error "Failure in the second PowerShell input was ignored"
  fi
  grep -q 'PSScriptAnalyzer reported issues' "$fixture/output" || log_error "Unexpected batch failure"

)}

test_staged_ci_guard_deletions()
{(
  log_stage "Checking full CI guards classify staged deletions and both rename paths"
  fixture="$DIR/.ci-guard-deletions-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir -p "$fixture/mock-bin" "$fixture/repo/cli" "$fixture/repo/conf" "$fixture/repo/symlinks" "$fixture/repo/docs" "$fixture/repo/hooks"
  cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = run ]
printf 'config\n' >> "$HOOK_INPUT_LOG"
while [ "$#" -gt 0 ]; do
  if [ "$1" = --root ]; then
    shift
    [ -f "$1/conf/required.toml" ] && [ -f "$1/symlinks/required" ]
    exit $?
  fi
  shift
done
exit 1
EOF
  cat > "$fixture/mock-bin/shellcheck" <<'EOF'
#!/bin/sh
echo "Deleted-only scripts must not be sent to ShellCheck" >&2
exit 1
EOF
  chmod +x "$fixture/mock-bin/"*
  PATH="$fixture/mock-bin:$PATH"
  HOOK_INPUT_LOG="$fixture/checks.log"
  DOTFILES_HOOKS_FULL=1
  export PATH HOOK_INPUT_LOG DOTFILES_HOOKS_FULL
  cd "$fixture/repo" || exit 1
  init_fixture_repository
  printf '[package]\nname = "fixture"\nversion = "0.1.0"\n' > cli/Cargo.toml
  printf 'required\n' > conf/required.toml
  printf 'required\n' > symlinks/required
  printf '#!/bin/sh\nexit 0\n' > hooks/removed.sh
  git add cli conf symlinks hooks
  git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null \
    commit -qm baseline

  for change in delete-config rename-config rename-source; do
    git reset --hard -q HEAD
    mkdir -p docs
    case "$change" in
      delete-config)
        git rm -q conf/required.toml
        # An unstaged restoration must not conceal the staged deletion.
        mkdir -p conf
        printf 'required\n' > conf/required.toml
        ;;
      rename-config) git mv conf/required.toml docs/required.md ;;
      rename-source) git mv symlinks/required docs/required.txt ;;
    esac
    : > "$HOOK_INPUT_LOG"
    if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
      log_error "Full guards accepted $change"
    fi
    [ "$(cat "$HOOK_INPUT_LOG")" = config ] || log_error "$change did not run config validation"
    grep -q 'configuration validation failed' "$fixture/output" ||
      log_error "$change failed for an unexpected reason"
  done

  git reset --hard -q HEAD
  git rm -q hooks/removed.sh
  sh "$DIR/hooks/check-ci-guards.sh"
)}

test_rust_and_shellcheck_inputs()
{

(
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
mkdir -p "$fixture/mock-bin" "$fixture/index repo/cli/src" "$fixture/index repo/conf"

cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' "$1" >> "$HOOK_INPUT_LOG"
if [ -n "${EXPECT_HOOK_CARGO_ENV:-}" ]; then
  [ -n "${DOTFILES_VERSION:-}" ]
  case "${CARGO_TARGET_DIR:-}" in
    */cli/target/hooks) ;;
    *) exit 1 ;;
  esac
fi
[ "$(cat src/lib.rs)" = valid-staged ]
[ "$(cat ../conf/fixture.toml)" = staged-config ]
[ ! -e src/untracked.rs ]
exit "${MOCK_CARGO_EXIT:-0}"
EOF
cat > "$fixture/mock-bin/rustc" <<'EOF'
#!/bin/sh
if [ "$1" = --version ]; then
  printf 'rustc %s (fixture)\n' "${MOCK_RUST_VERSION:-1.95.0}"
else
  exit 1
fi
EOF
cat > "$fixture/mock-bin/pwsh" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = -NoProfile ] && [ "$2" = -File ]
shift 3
[ "$#" -eq 1 ]
[ "$(cat "$1")" = valid-staged ]
printf 'powershell\n' >> "$HOOK_INPUT_LOG"
EOF
cat > "$fixture/mock-bin/shellcheck" <<'EOF'
#!/bin/sh
set -eu
for file do
  case "$file" in --*) continue ;; esac
  [ -f "$file" ] || { printf 'Missing ShellCheck input: %s\n' "$file" >&2; exit 2; }
  basename "$file" >> "$HOOK_INPUT_LOG"
done
exit "${SHELLCHECK_EXIT:-0}"
EOF
chmod +x "$fixture/mock-bin/"*
PATH="$fixture/mock-bin:$PATH"
export PATH
HOOK_INPUT_LOG="$fixture/checks.log"
export HOOK_INPUT_LOG

log_stage "Checking index-only Rust and PowerShell hook inputs"
(
  cd "$fixture/index repo" || exit 1
  init_fixture_repository
  printf '[package]\nname = "fixture"\nversion = "0.1.0"\n' > cli/Cargo.toml
  printf '[toolchain]\nchannel = "1.95.0"\n' > cli/rust-toolchain.toml
  printf 'invalid-staged\n' > cli/src/lib.rs
  printf 'valid-staged\n' > 'script with spaces.ps1'
  printf 'staged-config\n' > conf/fixture.toml
  git add cli conf 'script with spaces.ps1'
  printf 'valid-staged\n' > cli/src/lib.rs
  if sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
    log_error "Rust hook accepted invalid staged content hidden by an unstaged fix"
  fi
  grep -q 'Commit aborted' "$fixture/output" || log_error "Rust hook failure was not explained"

  git add cli/src/lib.rs
  printf 'invalid-unstaged\n' > cli/src/lib.rs
  printf 'unstaged-config\n' > conf/fixture.toml
  printf 'untracked\n' > cli/src/untracked.rs
  rm 'script with spaces.ps1'
  : > "$HOOK_INPUT_LOG"
  (
    unset CARGO_TARGET_DIR DOTFILES_VERSION
    EXPECT_HOOK_CARGO_ENV=1 DOTFILES_HOOKS_FULL=1 sh "$DIR/hooks/check-rust.sh"
  )
  expected=$(printf '%s\n' fmt clippy test powershell)
  [ "$(cat "$HOOK_INPUT_LOG")" = "$expected" ] || log_error "Not all checks ran against staged content"

  printf 'invalid-staged\n' > 'script with spaces.ps1'
  git add 'script with spaces.ps1'
  printf 'valid-staged\n' > 'script with spaces.ps1'
  if sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
    log_error "PowerShell hook accepted invalid staged content hidden by an unstaged fix"
  fi
  grep -q 'PSScriptAnalyzer reported issues' "$fixture/output" || log_error "PowerShell hook failure was not explained"
)

log_stage "Checking compiler pin and Cargo-only staged changes"
(
  cd "$fixture/index repo" || exit 1
  init_fixture_repository
  rm -f cli/src/untracked.rs
  # Restore the previously staged valid fixture; commit only inside this fixture.
  printf 'valid-staged\n' > cli/src/lib.rs
  printf 'valid-staged\n' > 'script with spaces.ps1'
  printf 'staged-config\n' > conf/fixture.toml
  git add cli conf 'script with spaces.ps1'
  git -c user.name=Fixture -c user.email=fixture@test.local commit -qm baseline
  for path in cli/Cargo.toml cli/Cargo.lock cli/rust-toolchain.toml cli/.cargo/config.toml .cargo/config.toml; do
    mkdir -p "$(dirname "$path")"
    printf '\n# staged change\n' >> "$path"
    git add "$path"
    : > "$HOOK_INPUT_LOG"
    sh "$DIR/hooks/check-rust.sh"
    [ "$(cat "$HOOK_INPUT_LOG")" = "$(printf '%s\n' fmt clippy)" ] ||
      log_error "Cargo-only change did not trigger checks: $path"
    git -c user.name=Fixture -c user.email=fixture@test.local commit -qm fixture
  done
  git rm -q cli/.cargo/config.toml
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-rust.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = "$(printf '%s\n' fmt clippy)" ] ||
    log_error "Deleted Cargo configuration did not trigger checks"
  : > "$HOOK_INPUT_LOG"
  if MOCK_RUST_VERSION=1.99.0 sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
    log_error "Hook accepted the wrong compiler"
  fi
  grep -q 'Pinned Rust 1.95.0 required' "$fixture/output" || log_error "Missing compiler mismatch explanation"
  [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Cargo ran before the compiler guard"
  # A working-tree pin must not override the staged pin.
  printf '[toolchain]\nchannel = "1.99.0"\n' > cli/rust-toolchain.toml
  sh "$DIR/hooks/check-rust.sh"
  git restore cli/rust-toolchain.toml
  git -c user.name=Fixture -c user.email=fixture@test.local commit -qm deletion
  git mv .cargo/config.toml .cargo/config
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-rust.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = "$(printf '%s\n' fmt clippy)" ] ||
    log_error "Renamed Cargo configuration did not trigger checks"
  git -c user.name=Fixture -c user.email=fixture@test.local commit -qm rename
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-rust.sh"
  [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Empty staged diff triggered Cargo"

  # Exercise canonical local stages using the same mock compiler and Cargo.
  mkdir -p .github/workflows/scripts/linux hooks
  cp "$DIR/.github/workflows/scripts/linux/check.sh" .github/workflows/scripts/linux/check.sh
  cp "$DIR/hooks/check-toolchain.sh" hooks/check-toolchain.sh
  : > "$HOOK_INPUT_LOG"
  sh .github/workflows/scripts/linux/check.sh fmt rustdoc
  [ "$(cat "$HOOK_INPUT_LOG")" = "$(printf '%s\n' fmt doc)" ] ||
    log_error "Local rustdoc stage did not invoke Cargo"
  : > "$HOOK_INPUT_LOG"
  if MOCK_RUST_VERSION=1.99.0 sh .github/workflows/scripts/linux/check.sh rustdoc > "$fixture/output" 2>&1; then
    log_error "Local check accepted the wrong compiler"
  fi
  [ ! -s "$HOOK_INPUT_LOG" ] || log_error "Local Cargo ran before the compiler guard"
  if MOCK_CARGO_EXIT=1 sh .github/workflows/scripts/linux/check.sh rustdoc > "$fixture/output" 2>&1; then
    log_error "Local check ignored a rustdoc failure"
  fi
)

log_stage "Checking ShellCheck paths containing spaces and glob characters"
mkdir -p "$fixture/shell repo [literal]/hooks" "$fixture/shell repo [literal]/symlinks"
(
  cd "$fixture/shell repo [literal]" || exit 1
  init_fixture_repository
  printf '#!/bin/sh\nexit 0\n' > 'hooks/one file.sh'
  printf '#!/bin/sh\nexit 0\n' > 'hooks/glob[script].sh'
  cp 'hooks/one file.sh' dotfiles.sh
  cp 'hooks/one file.sh' 'symlinks/another file.sh'
  git add hooks
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(sort "$HOOK_INPUT_LOG")" = "$(printf '%s\n' 'glob[script].sh' 'one file.sh')" ] ||
    log_error "Hook ShellCheck arguments did not preserve complete filenames"

  static_analysis="$DIR/.github/workflows/scripts/linux/test-static-analysis.sh"
  : > "$HOOK_INPUT_LOG"
  DIR="$PWD" sh "$static_analysis" test_shellcheck
  expected=$(printf '%s\n' 'another file.sh' dotfiles.sh 'glob[script].sh' 'one file.sh')
  [ "$(sort "$HOOK_INPUT_LOG")" = "$expected" ] ||
    log_error "Static-analysis ShellCheck arguments did not preserve complete filenames"

  if SHELLCHECK_EXIT=1 sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "Hook ignored a ShellCheck failure"
  fi
  if DIR="$PWD" SHELLCHECK_EXIT=1 sh "$static_analysis" test_shellcheck > "$fixture/output" 2>&1; then
    log_error "Static analysis ignored a ShellCheck failure"
  fi
)
log_verbose "Hook input regression tests passed"
)
}
