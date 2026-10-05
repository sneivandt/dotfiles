#!/bin/sh
# Shared discovery and invocation for the CLI, CI, and staged-file hooks.
set -eu

is_shell_script() {
  case "$1" in
    *.zsh) return 1 ;;
    *.sh) return 0 ;;
  esac
  LC_ALL=C awk '
    NR == 1 {
      if (substr($0, 1, 2) != "#!") exit 1
      sub(/^#![ \t]*/, "")
      count = split($0, words, /[ \t]+/)
      name = words[1]
      sub(/^.*\//, "", name)
      sub(/\.exe$/, "", name)
      if (name == "env") {
        name = ""
        for (i = 2; i <= count; i++) {
          if (words[i] !~ /^-/) { name = words[i]; break }
        }
        sub(/\.exe$/, "", name)
      }
      exit !(name ~ /^(sh|bash|dash|ksh)$/)
    }
    END { if (NR == 0) exit 1 }
  ' "$1"
}

collect_scripts() {
  [ -d "$1" ] && [ -r "$1" ] && [ -x "$1" ] || {
    printf 'Cannot read linter input directory: %s\n' "$1" >&2
    return 2
  }
  for entry in "$1"/* "$1"/.[!.]* "$1"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    if [ -d "$entry" ]; then
      collect_scripts "$entry" || return $?
    elif [ ! -f "$entry" ] || [ ! -r "$entry" ]; then
      printf 'Cannot read linter input: %s\n' "$entry" >&2
      return 2
    elif is_shell_script "$entry"; then
      printf '%s\0' "$entry"
    else
      status=$?
      [ "$status" -eq 1 ] || return "$status"
    fi
  done
}

inputs=$(mktemp)
trap 'rm -f "$inputs"' EXIT
trap 'exit 1' HUP INT TERM
if [ "${1:-}" = --root ]; then
  root=$2
  if [ -e "$root/dotfiles.sh" ] || [ -L "$root/dotfiles.sh" ]; then
    printf '%s\0' "$root/dotfiles.sh" > "$inputs"
  fi
  for dir in symlinks hooks .github cli/src/app/validation/scripts; do
    path="$root/$dir"
    if [ -e "$path" ] || [ -L "$path" ]; then
      collect_scripts "$path" >> "$inputs"
    fi
  done
elif [ "$#" -gt 0 ]; then
  printf '%s\0' "$@" > "$inputs"
fi
[ -s "$inputs" ] || exit 0
xargs -0 shellcheck --severity=warning \
  --exclude=SC1090,SC1091,SC3043,SC2154 \
  --enable=avoid-nullary-conditions -- < "$inputs"
