#!/usr/bin/env zsh

setopt PROMPT_SUBST

# Performance: Cache static values that don't change during session
typeset -g _ZSH_PROMPT_HOST=""
if [ -n "$SSH_CONNECTION" ] || [ -e /.dockerenv ]; then
  _ZSH_PROMPT_HOST="%F{cyan}%m%f "
fi

typeset -g _ZSH_PROMPT_SHELL=""
if [ "$(readlink -f "$(command -v zsh)")" != "$(readlink -f "$SHELL")" ]; then
  _ZSH_PROMPT_SHELL="%F{cyan}zsh%f "
fi

# Fast git prompt info
git_prompt_info()
{
  local current_branch
  current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null) || return
  if [ -n "$current_branch" ]; then
    # Keep branch-name percent signs from becoming zsh prompt escapes.
    current_branch=${current_branch//\%/%%}
    print -rn -- " %F{white}${current_branch}%f"
  fi
}

prompt_cmd()
{
  print -rn -- "%f${_ZSH_PROMPT_HOST}${_ZSH_PROMPT_SHELL}%F{yellow}%~%f$(git_prompt_info)%f"
}

PROMPT='$(prompt_cmd)
%(!.#.$) '
