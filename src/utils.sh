#!/bin/sh
set -o errexit
set -o nounset

# is_env_ignored
#
# Check if an environment is ignored.
#
# Args:
#     $1 - The environment to check.
#
# return:
#     bool - True if the environment is ignored.
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
  esac
  return 1
}

# is_flag_set
#
# Check if a flag is set.
#
# Args:
#     $1 - The flag to check.
#
# return:
#     bool - True if the flag is set.
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
# Check if a program is installed.
#
# Args:
#     $1 - The program to check.
#
# return:
#     bool - True if the program is installed.
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
# Check if a file is a shell script.
#
# Args:
#     $1 - The file to check.
#
# return:
#     bool - True if the file is a shell script.
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
# Check if a symlink is installed.
#
# Args:
#     $1 - The environment to be checked.
#     $2 - The symlink to be checked.
#
# return:
#     bool - True if the symlink is installed.
is_symlink_installed()
{
  if [ "$(readlink -f "$DIR"/env/"$1"/symlinks/"$2")" = "$(readlink -f ~/."$2")" ]
  then
    return 0
  else
    return 1
  fi
}