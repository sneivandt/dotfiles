#!/bin/sh
#
# Runs code quality checks on staged files:
#   1. cargo fmt --check                       (formatting, on .rs changes)
#   2. cargo clippy (host target)              (linting, on .rs changes)
#   3. cargo clippy --target x86_64-pc-windows-gnu
#                                              (cross-platform linting, full mode)
#   4. cargo test                              (unit/integration tests, full mode)
#   5. PSScriptAnalyzer                        (PowerShell linting, on .ps1/.psm1)
#
# Full-mode checks run only when DOTFILES_HOOKS_FULL=1 so ordinary commits stay
# fast. PSScriptAnalyzer is skipped (with a notice) when the required tooling is
# not installed locally.
#
# Can be run standalone or called from the pre-commit hook.
# Usage: sh check-rust.sh

set -o errexit
set -o nounset

RED=$(printf '\033[0;31m')
YELLOW=$(printf '\033[1;33m')
DIM=$(printf '\033[2m')
NC=$(printf '\033[0m')

WIN_TARGET="x86_64-pc-windows-gnu"

full_checks_enabled() {
  case "${DOTFILES_HOOKS_FULL:-0}" in
    1 | true | yes) return 0 ;;
    *) return 1 ;;
  esac
}

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

STAGED=$(git diff --cached --name-only --diff-filter=ACM "$against")

# ── Rust checks ────────────────────────────────────────
if printf '%s\n' "$STAGED" | grep -q '\.rs$'; then
  MANIFEST=$(git rev-parse --show-toplevel)/cli/Cargo.toml

  printf "Running cargo fmt --check...\n"
  if ! cargo fmt --manifest-path "$MANIFEST" --check 2>&1; then
    printf '\n%s======================================================%s\n' "$RED" "$NC"
    printf '%sCommit aborted: Rust files are not formatted.%s\n' "$RED" "$NC"
    printf '%sRun the following to fix:%s\n' "$YELLOW" "$NC"
    printf "  cargo fmt --manifest-path cli/Cargo.toml\n"
    printf '%sor bypass with:%s\n' "$YELLOW" "$NC"
    printf "  git commit --no-verify\n\n"
    exit 1
  fi

  printf "Running cargo clippy (host)...\n"
  if ! cargo clippy --profile ci --manifest-path "$MANIFEST" --all-targets -- -D warnings 2>&1; then
    printf '\n%s======================================================%s\n' "$RED" "$NC"
    printf '%sCommit aborted: cargo clippy reported warnings.%s\n' "$RED" "$NC"
    printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
    printf "  git commit --no-verify\n\n"
    exit 1
  fi

  if full_checks_enabled; then
    # Cross-target clippy: catches Windows-only cfg arms, missing imports under
    # #[cfg(windows)], winreg type errors, etc. Skipped when toolchain absent.
    if rustc --print target-list 2>/dev/null | grep -qx "$WIN_TARGET" \
       && rustc --print sysroot 2>/dev/null | xargs -I{} test -d "{}/lib/rustlib/$WIN_TARGET"; then
      printf "Running cargo clippy (%s)...\n" "$WIN_TARGET"
      if ! cargo clippy --profile ci --manifest-path "$MANIFEST" --target "$WIN_TARGET" --all-targets -- -D warnings 2>&1; then
        printf '\n%s======================================================%s\n' "$RED" "$NC"
        printf '%sCommit aborted: cargo clippy reported warnings on %s.%s\n' "$RED" "$WIN_TARGET" "$NC"
        printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
        printf "  git commit --no-verify\n\n"
        exit 1
      fi
    else
      printf '%sSkipping cross-target clippy (%s): toolchain not installed.%s\n' "$DIM" "$WIN_TARGET" "$NC"
      printf '%s  Install: rustup target add %s && pacman -S mingw-w64-gcc%s\n' "$DIM" "$WIN_TARGET" "$NC"
    fi

    printf "Running cargo test...\n"
    if ! cargo test --profile ci --manifest-path "$MANIFEST" 2>&1; then
      printf '\n%s======================================================%s\n' "$RED" "$NC"
      printf '%sCommit aborted: cargo test failed.%s\n' "$RED" "$NC"
      printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
      printf "  git commit --no-verify\n\n"
      exit 1
    fi
  else
    printf '%sSkipping full Rust hook checks: set DOTFILES_HOOKS_FULL=1 to run Windows clippy and cargo test.%s\n' "$DIM" "$NC"
  fi
fi

# ── PowerShell checks ─────────────────────────────────
if printf '%s\n' "$STAGED" | grep -qE '\.(ps1|psm1)$'; then
  if command -v pwsh >/dev/null 2>&1; then
    printf "Running PSScriptAnalyzer...\n"
    REPO_ROOT=$(git rev-parse --show-toplevel)
    PS_FILES=$(printf '%s\n' "$STAGED" | grep -E '\.(ps1|psm1)$' | sed "s|^|$REPO_ROOT/|" | tr '\n' ';')
    export PS_FILES
    # shellcheck disable=SC2016  # PowerShell variables are expanded by pwsh.
    if ! pwsh -NoProfile -Command '
      if (-not (Get-Module -ListAvailable -Name PSScriptAnalyzer)) {
        Write-Host "PSScriptAnalyzer module not installed; skipping." -ForegroundColor DarkGray
        Write-Host "  Install: pwsh -Command Install-Module PSScriptAnalyzer -Scope CurrentUser" -ForegroundColor DarkGray
        exit 0
      }
      Import-Module PSScriptAnalyzer -Force
      $hasErrors = $false
      $env:PS_FILES.Split(";") | Where-Object { $_ -ne "" } | ForEach-Object {
        $results = Invoke-ScriptAnalyzer -Path $_ -Severity Warning,Error
        if ($results) {
          $results | Format-Table -AutoSize
          $hasErrors = $true
        }
      }
      if ($hasErrors) { exit 1 }
    ' 2>&1; then
      printf '\n%s======================================================%s\n' "$RED" "$NC"
      printf '%sCommit aborted: PSScriptAnalyzer reported issues.%s\n' "$RED" "$NC"
      printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
      printf "  git commit --no-verify\n\n"
      exit 1
    fi
  else
    printf '%sSkipping PSScriptAnalyzer: pwsh not installed.%s\n' "$DIM" "$NC"
  fi
fi

exit 0
