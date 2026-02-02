#!/usr/bin/env zsh

setopt PROMPT_SUBST

# Performance: Cache static values that don't change during session
typeset -g _ZSH_PROMPT_HOST=""
if [ -n "$SSH_CONNECTION" ] || [ -e /.dockerenv ]; then
  _ZSH_PROMPT_HOST="%{$fg[cyan]%}%m%{$reset_color%} "
fi

typeset -g _ZSH_PROMPT_SHELL=""
if [ "$(command -vp zsh)" != "$SHELL" ]; then
  _ZSH_PROMPT_SHELL="%{$fg[cyan]%}zsh%{$reset_color%} "
fi

# Fast git prompt info
git_prompt_info()
{
  # Performance: Fast check if in git repo
  if ! git rev-parse --git-dir >/dev/null 2>&1; then
    return
  fi

  local current_branch
  current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
  if [ -n "$current_branch" ]; then
    echo -n " %{$fg[white]%}${current_branch}%{$reset_color%}"
    # Performance: Use --porcelain=v1 and --untracked-files=no for speed
    local dirty=$(git --no-optional-locks status --porcelain=v1 --untracked-files=no 2>/dev/null | wc -l)
    if [ $dirty -gt 0 ]; then
      echo -n "%{$fg[red]%}+${dirty// /}%{$reset_color%}"
    fi
  fi
}

# Fast sudo check
sudo_active()
{
  # Performance: Use sudo -n true instead of uptime
  if sudo -n true 2>/dev/null; then
    echo -n " %{$fg[cyan]%}!%{$reset_color%}"
  fi
}

# Optimized prompt command
prompt_cmd()
{
  echo -n "%{$reset_color%}${_ZSH_PROMPT_HOST}${_ZSH_PROMPT_SHELL}%{$fg[yellow]%}%~%{$reset_color%}$(git_prompt_info)$(sudo_active)%{$reset_color%}"
}

PROMPT='$(prompt_cmd)
%(!.#.$) '
