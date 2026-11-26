#!/bin/sh
set -eu

version=1.7.12
archive="actionlint_${version}_linux_amd64.tar.gz"
sha256=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8
url="https://github.com/rhysd/actionlint/releases/download/v${version}/${archive}"
install_dir=$(mktemp -d)

curl --fail --location --retry 3 "$url" -o "$install_dir/$archive"
printf '%s  %s\n' "$sha256" "$install_dir/$archive" | sha256sum --check -
tar -xzf "$install_dir/$archive" -C "$install_dir" actionlint
printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
