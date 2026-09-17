#!/bin/sh
set -o errexit
set -o nounset

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR/lib/test-helpers.sh"
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../../../.." && pwd)}

fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
mkdir -p "$fixture/mock-bin" "$fixture/index repo/cli/src" "$fixture/index repo/conf"

cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' "$1" >> "$HOOK_INPUT_LOG"
[ "$(cat src/lib.rs)" = valid-staged ]
[ "$(cat ../conf/fixture.toml)" = staged-config ]
[ ! -e src/untracked.rs ]
EOF
cat > "$fixture/mock-bin/rustc" <<'EOF'
#!/bin/sh
exit 1
EOF
cat > "$fixture/mock-bin/pwsh" <<'EOF'
#!/bin/sh
set -eu
file=${PS_FILES%;}
[ "$(cat "$file")" = valid-staged ]
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
  cd "$fixture/index repo"
  git -c init.templateDir= init -q
  printf '[package]\nname = "fixture"\nversion = "0.1.0"\n' > cli/Cargo.toml
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
  DOTFILES_HOOKS_FULL=1 sh "$DIR/hooks/check-rust.sh"
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

log_stage "Checking ShellCheck paths containing spaces and glob characters"
mkdir -p "$fixture/shell repo [literal]/hooks" "$fixture/shell repo [literal]/symlinks"
(
  cd "$fixture/shell repo [literal]"
  git -c init.templateDir= init -q
  printf '#!/bin/sh\nexit 0\n' > 'hooks/one file.sh'
  printf '#!/bin/sh\nexit 0\n' > 'hooks/glob[script].sh'
  cp 'hooks/one file.sh' dotfiles.sh
  cp 'hooks/one file.sh' 'symlinks/another file.sh'
  git add hooks
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(sort "$HOOK_INPUT_LOG")" = "$(printf '%s\n' 'glob[script].sh' 'one file.sh')" ] ||
    log_error "Hook ShellCheck arguments did not preserve complete filenames"

  : > "$HOOK_INPUT_LOG"
  DIR="$PWD" sh "$SCRIPT_DIR/test-static-analysis.sh" test_shellcheck
  expected=$(printf '%s\n' 'another file.sh' dotfiles.sh 'glob[script].sh' 'one file.sh')
  [ "$(sort "$HOOK_INPUT_LOG")" = "$expected" ] ||
    log_error "Static-analysis ShellCheck arguments did not preserve complete filenames"

  if SHELLCHECK_EXIT=1 sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "Hook ignored a ShellCheck failure"
  fi
  if DIR="$PWD" SHELLCHECK_EXIT=1 sh "$SCRIPT_DIR/test-static-analysis.sh" test_shellcheck > "$fixture/output" 2>&1; then
    log_error "Static analysis ignored a ShellCheck failure"
  fi
)
log_verbose "Hook input regression tests passed"
