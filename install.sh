#!/bin/sh

# https://docs.github.com/en/codespaces/customizing-your-codespace/personalizing-github-codespaces-for-your-account#dotfiles

# Install dotfiles
./dotfiles.sh -Ig

# Configure vscode remote
mkdir -pv ~/.vscode-remote/data/Machine/
rm -rvf ~/.vscode-remote/data/Machine/settings.json
ln -snvf /workspaces/.codespaces/.persistedshare/dotfiles/env/base-gui/symlinks/config/Code/User/settings.json ~/.vscode-remote/data/Machine/settings.json