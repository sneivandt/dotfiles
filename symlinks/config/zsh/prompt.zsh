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
  # Performance: Fast check if in git repo
  if ! git rev-parse --git-dir >/dev/null 2>&1; then
    return
  fi

  local current_branch
  current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
  if [ -n "$current_branch" ]; then
    echo -n " %F{white}${current_branch}%f"
    # Performance: Use --porcelain=v1 and --untracked-files=no for speed
    local changes=$(git --no-optional-locks status --porcelain=v1 --untracked-files=no 2>/dev/null | wc -l)
    if [ "${changes:-0}" -gt 0 ]; then
      echo -n "%F{red}+${changes// /}%f"
    fi
  fi
}

# Fast sudo check
sudo_active()
{
  # Performance: Use sudo -n true instead of uptime
  if sudo -n true 2>/dev/null; then
    echo -n " %F{cyan}!%f"
  fi
}

prompt_cmd()
{
  echo -n "%f${_ZSH_PROMPT_HOST}${_ZSH_PROMPT_SHELL}%F{yellow}%~%f$(git_prompt_info)$(sudo_active)%f"
}

PROMPT='$(prompt_cmd)
%(!.#.$) '
