#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# utils.sh
# -----------------------------------------------------------------------------
# Small predicate / helper functions consumed by task + command layers.
# Each returns 0 for "true" (POSIX convention) and 1 for "false".
# Keep logic minimal—complex workflows belong in tasks.sh.
# -----------------------------------------------------------------------------

# Detect host OS once to avoid repeated expensive checks.
# IS_ARCH: 1 (processed) if Arch Linux / Arch-based, 0 (ignored) otherwise.
# Logic: Check /etc/*-release. If ANY match for "arch" or "archlinux" is found
# in ID fields, we consider it Arch.
IS_ARCH=0
if grep -E "^(ID|ID_LIKE)=.*" /etc/*-release 2>/dev/null | cut -d= -f2 | tr -d '"' | grep -qxE "arch|archlinux"
then
  IS_ARCH=1
fi

# is_env_ignored
#
# Returns success if the named environment directory should be skipped based
# on host OS, selected CLI flags, or composed dependencies. Environments may
# layer (e.g., arch-gui depends on arch + base-gui). This function encodes
# those dependency rules centrally so callers just iterate env/* and test.
#
# Args:
#   $1  environment name (basename of env/<name>)
#
# Result:
#   0 ignored / skip, 1 process.
is_env_ignored()
{
  # Early return for win - always ignored
  [ "$1" = "win" ] && return 0
  
  case $1 in
    arch)
      # If not on Arch (IS_ARCH=0), ignore it (return 0)
      [ "$IS_ARCH" -eq 0 ] && return 0
      ;;
    arch-gui)
      is_env_ignored "base-gui" && return 0
      is_env_ignored "arch" && return 0
      ;;
    base-gui)
      ! is_flag_set "g" && return 0
      ;;
  esac
  return 1
}

# is_flag_set
#
# Check whether a short flag (single character) was present in the original
# CLI invocation as normalized by getopt and stored in $OPT.
#
# Args:
#   $1  single-letter flag (without leading dash)
#
# Result:
#   0 flag present, 1 absent.
is_flag_set()
{
  case " $OPT " in
    *" -$1 "*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# is_program_installed
#
# Predicate for presence of an executable in PATH. Use `command -v` to avoid
# subshells and output capture overhead.
#
# Args:
#   $1 program name
#
# Result:
#   0 found, 1 missing.
is_program_installed()
{
  if command -v "$1" >/dev/null 2>&1
  then
    return 0
  else
    return 1
  fi
}

# is_shell_script
#
# Heuristic: file exists and first line shebang references a POSIX / bash shell.
# Avoids false positives on plain text without execution semantics.
#
# Args:
#   $1 path to file
#
# Result:
#   0 matches known shell shebang, 1 otherwise.
is_shell_script()
{
  if [ -f "$1" ]
  then
    case "$(head -n 1 "$1")" in
      '#!/bin/sh'* | '#!/bin/bash'* | '#!/usr/bin/env sh'* | '#!/usr/bin/env bash'*)
        return 0
        ;;
    esac
  fi
  return 1
}

# is_symlink_installed
#
# Compare resolved target of managed symlink against existing entry in $HOME.
# Ensures we only re-link when drift occurred. Uses readlink -f for canonical
# path resolution (following any intermediate symlinks).
#
# Args:
#   $1 environment name
#   $2 relative symlink path as listed in symlinks.conf
#
# Result:
#   0 installed & matches, 1 absent or different.
is_symlink_installed()
{
  # shellcheck disable=SC2012
  if [ "$(readlink -f "$DIR"/env/"$1"/symlinks/"$2")" = "$(readlink -f ~/."$2")" ]
  then
    return 0
  else
    return 1
  fi
}

# Program cache for is_program_installed_cached
_program_cache=""

# is_program_installed_cached
#
# Cached version of is_program_installed. Stores successful lookups in a
# string to avoid repeated command -v calls. Useful when checking the same
# program multiple times across different layers or iterations.
# Uses pipe delimiters and exact matching to prevent false positives.
#
# Args:
#   $1 program name
#
# Result:
#   0 found (in cache or via command -v), 1 missing.
is_program_installed_cached()
{
  # Check if already in cache (exact match with surrounding pipes)
  case "|$_program_cache|" in
    *"|$1|"*) return 0 ;;
  esac
  
  # Not in cache, check if installed
  if is_program_installed "$1"
  then
    _program_cache="$_program_cache|$1"
    return 0
  fi
  return 1
}
