#!/usr/bin/env zsh

setopt PROMPT_SUBST

# Performance: Cache static values that don't change during session
typeset -g _ZSH_PROMPT_HOST=""
if [ -n "${SSH_CONNECTION:-}" ] || [ -e /.dockerenv ]; then
  _ZSH_PROMPT_HOST="%F{cyan}%m%f "
fi

typeset -g _ZSH_PROMPT_SHELL=""
# Compare physical paths so a symlink to the default shell doesn't add a label.
if [[ ${${commands[zsh]-}:P} != ${${SHELL:-}:P} ]]; then
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

PROMPT='%f${_ZSH_PROMPT_HOST}${_ZSH_PROMPT_SHELL}%F{yellow}%~%f$(git_prompt_info)%f
%(!.#.$) '
