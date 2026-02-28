#!/bin/sh
# shellcheck disable=SC2030,SC2031
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-paru.sh — Paru (AUR helper) installation and functionality tests.
# Called directly from CI in an Arch Linux container.
# Usage: test-paru.sh <repo-dir>
# -----------------------------------------------------------------------------

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <repo-dir>" >&2
  exit 1
fi

PATH="/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin:$PATH"
export PATH

DIR="$1"
export DIR
cd "$DIR"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

do_install_paru() {
  if is_program_installed "paru"; then
    log_verbose "Paru already installed, skipping"
    return 0
  fi
  log_stage "Installing paru (AUR helper)"
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT
  retry_cmd 3 15 git clone --depth 1 https://aur.archlinux.org/paru-git.git "$tmp_dir"
  cd "$tmp_dir"
  MAKEFLAGS="-j$(nproc)"
  export MAKEFLAGS
  retry_cmd 3 15 makepkg -si --noconfirm
}

# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

test_paru_prerequisites()
{(
  log_stage "Testing paru prerequisites"
  missing=0
  for prog in git makepkg cargo; do
    if is_program_installed "$prog"; then
      log_verbose "✓ $prog installed"
    else
      printf "%sERROR: prerequisite '%s' missing%s\n" "${RED}" "$prog" "${NC}" >&2
      missing=$((missing + 1))
    fi
  done
  [ "$missing" -eq 0 ] || return 1
)}

test_paru_install()
{(
  log_stage "Testing paru installation"
  do_install_paru
  is_program_installed "paru" || { printf "%sERROR: paru not found after install%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Paru installed successfully"
)}

test_paru_available()
{(
  log_stage "Testing paru availability"
  is_program_installed "paru" || { printf "%sERROR: paru not installed%s\n" "${RED}" "${NC}" >&2; return 1; }
  paru --version >/dev/null 2>&1 || { printf "%sERROR: paru --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Paru version: $(paru --version 2>&1 | head -n1)"
  paru --help >/dev/null 2>&1 || { printf "%sERROR: paru --help failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Paru is functional"
)}

test_aur_packages()
{(
  log_stage "Testing AUR package search"
  is_program_installed "paru" || { log_verbose "Skipping: paru not installed"; return 0; }
  if paru -Ss --noconfirm base-devel >/dev/null 2>&1; then
    log_verbose "✓ Paru package search works"
  else
    printf "%sERROR: paru search failed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
)}

test_paru_config()
{(
  log_stage "Testing paru configuration"
  is_program_installed "paru" || { log_verbose "Skipping: paru not installed"; return 0; }
  for cfg in /etc/paru.conf "$HOME/.config/paru/paru.conf"; do
    [ -f "$cfg" ] && log_verbose "Found config: $cfg"
  done
  paru --version >/dev/null 2>&1 || { printf "%sERROR: paru --version failed, config may have issues%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Paru configuration OK"
)}

test_paru_idempotency()
{(
  log_stage "Testing paru idempotency"
  do_install_paru
  log_verbose "First run done"
  do_install_paru
  log_verbose "Second run done (should have skipped)"
  is_program_installed "paru" || { printf "%sERROR: paru missing after idempotency test%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Idempotency test passed"
)}

# ---------------------------------------------------------------------------
# Run all tests
# ---------------------------------------------------------------------------

test_paru_prerequisites
test_paru_install
test_paru_available
test_aur_packages
test_paru_config
test_paru_idempotency

echo "All paru tests passed"
