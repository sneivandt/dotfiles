#!/bin/sh
#
# check.sh — canonical local verification entry point.
#
# Runs the same checks CI runs, in one command, so "passed locally" and
# "passed in CI" mean the same thing. Every stage is optional-tool aware: if a
# stage's tool is not installed it is reported as SKIP rather than failing, so
# the script stays usable on a minimal machine.
#
# Usage:
#   sh .github/workflows/scripts/linux/check.sh                 # run the default stages
#   sh .github/workflows/scripts/linux/check.sh fmt clippy      # run only the named stages
#   sh .github/workflows/scripts/linux/check.sh --all           # include opt-in stages (msrv)
#   sh .github/workflows/scripts/linux/check.sh --list          # list available stages
#
# Exit status is non-zero if any stage failed.
#
# The PowerShell twin is .github/workflows/scripts/windows/Check.ps1.

set -o nounset

REPO_ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../../../.." && pwd)
MANIFEST="$REPO_ROOT/cli/Cargo.toml"

# Match the profile CI builds with so local failures reproduce CI failures.
CARGO_PROFILE=ci

# Stages run when no stage arguments are given.
DEFAULT_STAGES="fmt clippy test config shell powershell audit deny"

# Stages only run when explicitly requested or via --all. `msrv` downloads a
# second toolchain, which is too slow for the default loop.
OPTIONAL_STAGES="msrv"

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_RED=$(printf '\033[31m')
  C_GREEN=$(printf '\033[32m')
  C_YELLOW=$(printf '\033[33m')
  C_BLUE=$(printf '\033[34m')
  C_DIM=$(printf '\033[2m')
  C_OFF=$(printf '\033[0m')
else
  C_RED=''
  C_GREEN=''
  C_YELLOW=''
  C_BLUE=''
  C_DIM=''
  C_OFF=''
fi

# Accumulated "<stage> <status>" lines, printed as a summary at the end.
RESULTS=''
FAILED=0

have()
{
  command -v "$1" >/dev/null 2>&1
}

heading()
{
  printf '\n%s== %s ==%s\n' "$C_BLUE" "$1" "$C_OFF"
}

note()
{
  printf '%s%s%s\n' "$C_DIM" "$1" "$C_OFF"
}

record()
{
  RESULTS="$RESULTS$1 $2
"
}

# Run a stage function and record pass/fail/skip.
#
# Stage functions return 0 for pass, 2 for skip, and anything else for fail.
run_stage()
{
  heading "$1"
  "stage_$1"
  case $? in
    0)
      record "$1" pass
      ;;
    2)
      record "$1" skip
      ;;
    *)
      record "$1" FAIL
      FAILED=1
      ;;
  esac
}

# ── Stages ────────────────────────────────────────────

stage_fmt()
{
  have cargo || { note "cargo not installed; skipping"; return 2; }
  cargo fmt --check --manifest-path "$MANIFEST"
}

stage_clippy()
{
  have cargo || { note "cargo not installed; skipping"; return 2; }
  cargo clippy --profile "$CARGO_PROFILE" --manifest-path "$MANIFEST" \
    --all-targets -- -D warnings
}

stage_test()
{
  have cargo || { note "cargo not installed; skipping"; return 2; }
  cargo test --profile "$CARGO_PROFILE" --manifest-path "$MANIFEST"
}

# Runs the CLI's own repository validator against this working tree.
stage_config()
{
  have cargo || { note "cargo not installed; skipping"; return 2; }
  cargo run --profile "$CARGO_PROFILE" --manifest-path "$MANIFEST" -- \
    --root "$REPO_ROOT" test
}

stage_shell()
{
  have shellcheck || { note "shellcheck not installed; skipping"; return 2; }
  DIR="$REPO_ROOT"
  export DIR
  ( cd "$REPO_ROOT/.github/workflows/scripts/linux" \
    && sh test-static-analysis.sh test_shellcheck )
}

stage_powershell()
{
  have pwsh || { note "pwsh not installed; skipping"; return 2; }
  DIR="$REPO_ROOT"
  export DIR
  ( cd "$REPO_ROOT/.github/workflows/scripts/linux" \
    && sh test-static-analysis.sh test_psscriptanalyzer )
}

stage_audit()
{
  have cargo || { note "cargo not installed; skipping"; return 2; }
  if ! cargo audit --version >/dev/null 2>&1; then
    note "cargo-audit not installed; skipping (cargo install cargo-audit)"
    return 2
  fi
  cargo audit --file "$REPO_ROOT/cli/Cargo.lock"
}

stage_deny()
{
  have cargo || { note "cargo not installed; skipping"; return 2; }
  if ! cargo deny --version >/dev/null 2>&1; then
    note "cargo-deny not installed; skipping (cargo install cargo-deny)"
    return 2
  fi
  cargo deny --manifest-path "$MANIFEST" check all
}

# Checks the crate still compiles on the declared minimum toolchain.
stage_msrv()
{
  have rustup || { note "rustup not installed; skipping"; return 2; }
  msrv=$(sed -n 's/^rust-version *= *"\([^"]*\)".*/\1/p' "$MANIFEST" | head -n 1)
  if [ -z "$msrv" ]; then
    note "no rust-version in $MANIFEST; skipping"
    return 2
  fi
  note "MSRV from Cargo.toml: $msrv"
  rustup toolchain install "$msrv" --profile minimal >/dev/null 2>&1 || true
  cargo "+$msrv" check --manifest-path "$MANIFEST" --all-targets
}

# ── Argument handling ─────────────────────────────────

usage()
{
  cat <<EOF
Usage: sh .github/workflows/scripts/linux/check.sh [--all | --list | STAGE...]

Stages (default): $DEFAULT_STAGES
Stages (opt-in):  $OPTIONAL_STAGES

Options:
  --all     Run every stage, including opt-in stages.
  --list    List available stages and exit.
  -h        Show this help and exit.
EOF
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  --list)
    for stage in $DEFAULT_STAGES $OPTIONAL_STAGES; do
      printf '%s\n' "$stage"
    done
    exit 0
    ;;
  --all)
    stages="$DEFAULT_STAGES $OPTIONAL_STAGES"
    ;;
  '')
    stages="$DEFAULT_STAGES"
    ;;
  *)
    stages="$*"
    ;;
esac

# Reject unknown stage names up front so a typo fails loudly instead of
# silently running fewer checks than the caller expects.
for stage in $stages; do
  case " $DEFAULT_STAGES $OPTIONAL_STAGES " in
    *" $stage "*) ;;
    *)
      printf '%sUnknown stage: %s%s\n\n' "$C_RED" "$stage" "$C_OFF" >&2
      usage >&2
      exit 2
      ;;
  esac
done

for stage in $stages; do
  run_stage "$stage"
done

# ── Summary ───────────────────────────────────────────

printf '\n%s== summary ==%s\n' "$C_BLUE" "$C_OFF"
printf '%s' "$RESULTS" | while read -r name status; do
  [ -n "$name" ] || continue
  case "$status" in
    pass) printf '  %sPASS%s %s\n' "$C_GREEN" "$C_OFF" "$name" ;;
    skip) printf '  %sSKIP%s %s\n' "$C_YELLOW" "$C_OFF" "$name" ;;
    *) printf '  %sFAIL%s %s\n' "$C_RED" "$C_OFF" "$name" ;;
  esac
done

if [ "$FAILED" -ne 0 ]; then
  printf '\n%sChecks failed.%s\n' "$C_RED" "$C_OFF" >&2
  exit 1
fi

printf '\n%sAll checks passed.%s\n' "$C_GREEN" "$C_OFF"
exit 0
