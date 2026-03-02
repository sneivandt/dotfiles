#!/bin/sh
set -o errexit
set -o nounset

# GitHub Codespaces installation script
# https://docs.github.com/en/codespaces/customizing-your-codespace/personalizing-github-codespaces-for-your-account#dotfiles
#
# Note: --build is intentionally omitted here. dotfiles.sh will download the
# latest pre-built binary from GitHub Releases automatically (with retry and
# checksum verification). Use --build only if no published release is available
# or if you need to test local source changes.

./dotfiles.sh install -p desktop