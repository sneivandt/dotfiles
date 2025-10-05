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
  case $1 in
    arch)
      if grep -xP "ID=.*|ID_LIKE=.*" /etc/*-release | cut -d= -f2 | grep -qvxP "arch|archlinux"
      then
        return 0
      fi
      ;;
    arch-gui)
      if is_env_ignored "base-gui" \
        || is_env_ignored "arch"
      then
        return 0
      fi
      ;;
    base-gui)
      if ! is_flag_set "g"
      then
        return 0
      fi
      ;;
    win)
      return 0
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
# Predicate for presence of an executable in PATH. Uses `command -vp` which
# resolves shell builtins and provides absolute path for determinism.
#
# Args:
#   $1 program name
#
# Result:
#   0 found, 1 missing.
is_program_installed()
{
  if [ -n "$(command -vp "$1")" ]
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
  if [ "$(readlink -f "$DIR"/env/"$1"/symlinks/"$2")" = "$(readlink -f ~/."$2")" ]
  then
    return 0
  else
    return 1
  fi
}