#!/usr/bin/env bash
# shellcheck shell=bash

# Performance: Cache static checks that don't change during session
_BASH_PROMPT_SSH=""
if [ -n "$SSH_CONNECTION" ] || [ -e /.dockerenv ]; then
  _BASH_PROMPT_SSH="$HOSTNAME "
fi

_BASH_PROMPT_SHELL=""
if [ "$(readlink -f "$(command -v bash)")" != "$(readlink -f "$SHELL")" ]; then
  _BASH_PROMPT_SHELL="bash "
fi

# Fast git prompt info
# Kept deliberately in sync with prompt.zsh: branch name only. A per-prompt
# `git status` is too expensive in large working trees.
__bash_git_prompt()
{
  local branch
  # Fast check: only run if in git repo
  if ! git rev-parse --git-dir >/dev/null 2>&1; then
    return
  fi

  branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
  if [ -n "$branch" ]; then
    printf " %s" "$branch"
  fi
}

# Check sudo status (cached per prompt)
# Uses `sudo -nv` (validate-only): unlike `sudo -n true`, it does not write a
# "a password is required" entry to the auth log when no timestamp is cached.
__bash_sudo_prompt()
{
  if sudo -nv 2>/dev/null; then
    printf " \001\e[0;36m\002!\001\e[0m\002"
  fi
}

PS1=""

# host name (cached)
PS1+="\\[\\e[0;36m\\]$_BASH_PROMPT_SSH"

# default shell (cached)
PS1+="\\[\\e[0;36m\\]$_BASH_PROMPT_SHELL"

# working dir
PS1+="\\[\\e[0;33m\\]\\w\\[\\e[0m\\]"

# git prompt info (function call for dynamic content)
PS1+="\$(__bash_git_prompt)"

# sudo active (function call for dynamic content)
PS1+="\$(__bash_sudo_prompt)"

# prompt
PS1+="\\n\\$ "
