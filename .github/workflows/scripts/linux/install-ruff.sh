#!/bin/sh
set -eu

version=0.17.6
target=x86_64-unknown-linux-gnu
archive="ruff-${target}.tar.gz"
sha256=94b33791f48cd54ab35ad673a443a5c001d3e6d87effeaba9899070b700d45f3
url="https://github.com/astral-sh/ruff/releases/download/${version}/${archive}"
install_dir=$(mktemp -d)

curl --fail --location --retry 3 "$url" -o "$install_dir/$archive"
printf '%s  %s\n' "$sha256" "$install_dir/$archive" | sha256sum --check -
tar -xzf "$install_dir/$archive" --strip-components=1 -C "$install_dir" "ruff-${target}/ruff"
printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
