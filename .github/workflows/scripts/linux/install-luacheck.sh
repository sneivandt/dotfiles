#!/bin/sh
set -eu

version=v1.2.0
sha256=d68da17fca0697d9e2fb04201f3884abd259fa558b3a449bccaed47f1390defc
url="https://github.com/lunarmodules/luacheck/releases/download/${version}/luacheck"
install_dir=$(mktemp -d)

curl --fail --location --retry 3 "$url" -o "$install_dir/luacheck"
printf '%s  %s\n' "$sha256" "$install_dir/luacheck" | sha256sum --check -
chmod +x "$install_dir/luacheck"
printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
