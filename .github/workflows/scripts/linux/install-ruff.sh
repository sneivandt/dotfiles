#!/bin/sh
set -eu

version=0.17.6
target=x86_64-unknown-linux-gnu
archive="ruff-${target}.tar.gz"
checksum="${archive}.sha256"
base_url="https://github.com/astral-sh/ruff/releases/download/${version}"
install_dir=$(mktemp -d)

curl --fail --location --retry 3 "$base_url/$archive" -o "$install_dir/$archive"
curl --fail --location --retry 3 "$base_url/$checksum" -o "$install_dir/$checksum"
(cd "$install_dir" && sha256sum --check "$checksum")
tar -xzf "$install_dir/$archive" --strip-components=1 -C "$install_dir" "ruff-${target}/ruff"
printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
