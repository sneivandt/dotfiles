#!/bin/sh
set -o errexit
set -o nounset

# GitHub Codespaces installation script
# https://docs.github.com/en/codespaces/customizing-your-codespace/personalizing-github-codespaces-for-your-account#dotfiles

./dotfiles.sh --build install -p desktop