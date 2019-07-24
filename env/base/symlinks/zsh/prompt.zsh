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
    echo -n " %{$fg[white]%}%{$current_branch%}%{$reset_color%}"
    if [ $(git status --short | wc -l) -gt 0 ]
    then
      echo -n "%{$fg[red]%}+$(git status --short | wc -l | awk '{$1=$1};1')%{$reset_color%}"
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
  print -rP "%{$reset_color%}$(working_dir)"
}

sprompt_cmd()
{
  print -rP "%{$reset_color%}$(host_name)$(default_shell)$(working_dir)$(git_prompt_info)$(sudo_active)"
}

PROMPT='$(prompt_cmd)
%(!.#.$) '

ASYNC_PROC=0
function precmd()
{
  function async()
  {
    mkdir -p ~/tmp && printf "%s" "$(sprompt_cmd)" > ~/tmp/.zsh_prompt
    kill -s USR1 $$
  }

  if [ "${ASYNC_PROC}" != 0 ]
  then
    kill -s HUP $ASYNC_PROC >/dev/null 2>&1 || :
  fi

  async &!
  ASYNC_PROC=$!
}

function TRAPUSR1()
{
  PROMPT='$(cat ~/tmp/.zsh_prompt 2>/dev/null)
%(!.#.$) '
  ASYNC_PROC=0
  zle && zle reset-prompt
}
