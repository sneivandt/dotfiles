#!/bin/sh
# Resolve/check release tags without treating remote errors as missing tags.
set -eu

tag_exists()
{
  if git ls-remote --exit-code --tags origin "refs/tags/$1" >/dev/null; then
    return 0
  else
    status=$?
    if [ "$status" -eq 2 ]; then
      return 1
    fi
    printf 'ERROR: Cannot query release tag %s (git status %s).\n' "$1" "$status" >&2
    exit "$status"
  fi
}

case "$1" in
  next)
    n=1
    while tag_exists "v$2-$n"; do
      n=$((n + 1))
    done
    printf 'v%s-%s\n' "$2" "$n"
    ;;
  check)
    if tag_exists "$2"; then
      printf 'Release tag %s already exists.\n' "$2" >&2
      exit 1
    fi
    ;;
  *) printf 'Usage: release-tag.sh next <date> | check <tag>\n' >&2; exit 2 ;;
esac
