#!/usr/bin/env zsh

setopt PROMPT_SUBST

host_name()
{
  if [ -n "$SSH_CONNECTION" ] || [ -e /.dockerenv ]
  then
    echo -n "%{$fg[cyan]%}%m%{$reset_color%} "
  fi
}

default_shell()
{
  if [ $(command -vp zsh) != $SHELL ]
  then
    echo -n "%{$fg[cyan]%}zsh%{$reset_color%} "
  fi
}

working_dir()
{
  echo -n "%{$fg[yellow]%}%~%{$reset_color%}"
}

git_prompt_info()
{
  current_branch=$(git rev-parse --abbrev-ref HEAD 2> /dev/null)
  if [ -n "$current_branch" ]
  then
    echo -n " %{$fg[white]%}${current_branch}%{$reset_color%}"
    local dirty=$(git status --short 2> /dev/null | wc -l)
    if [ $dirty -gt 0 ]
    then
      echo -n "%{$fg[red]%}+${dirty// /}%{$reset_color%}"
    fi
  fi
}

sudo_active()
{
  can_sudo=$(sudo -n uptime 2>&1 | grep -c "load")
  if [ ${can_sudo} -gt 0 ]
  then
    echo -n " %{$fg[cyan]%}!%{$reset_color%}"
  fi
}

prompt_cmd()
{
  echo -n "%{$reset_color%}$(host_name)$(default_shell)$(working_dir)$(git_prompt_info)$(sudo_active)%{$reset_color%}"
}

PROMPT='$(prompt_cmd)
%(!.#.$) '
