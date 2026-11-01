#!/bin/sh
# Verify PATH uses the repository compiler before running pinned local checks.
set -eu
cli_root=$1
expected=$(sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' "$cli_root/rust-toolchain.toml")
if [ -z "$expected" ]; then
  printf 'Cannot read pinned Rust version from %s/rust-toolchain.toml\n' "$cli_root" >&2
  exit 1
fi
actual=$(cd "$cli_root" && rustc --version) || exit 1
case "$actual" in
  "rustc $expected "*) ;;
  *)
    printf 'Pinned Rust %s required; PATH selected %s.\n' "$expected" "$actual" >&2
    printf 'Select the pinned toolchain as described in docs/content/contributing.md#selecting-the-pinned-toolchain.\n' >&2
    exit 1
    ;;
esac
