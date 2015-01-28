function ssh_connection() {
  if [[ -n $SSH_CONNECTION ]]
  then
    echo "%{$fg[cyan]%}%m "
  fi
}

PROMPT='$(ssh_connection)%{$fg[yellow]%}%~$(git_prompt_info)
%{$reset_color%}%(!.#.$) '

ZSH_THEME_GIT_PROMPT_PREFIX=" %{$reset_color%}"
ZSH_THEME_GIT_PROMPT_SUFFIX="%{$reset_color%}"
ZSH_THEME_GIT_PROMPT_CLEAN="%{$fg[green]%} ✔"
ZSH_THEME_GIT_PROMPT_DIRTY="%{$fg[red]%} ✗"
