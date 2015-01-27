function ssh_connection() {
  if [[ -n $SSH_CONNECTION ]]
  then
    echo "%{$fg[blue]%}%m "
  fi
}

PROMPT=$'$(ssh_connection)%{$fg[yellow]%}%(!.%1~.%~)$(git_prompt_info)\n%{$fg[red]%}➜%{$reset_color%} '

ZSH_THEME_GIT_PROMPT_PREFIX=" %{$reset_color%}"
ZSH_THEME_GIT_PROMPT_SUFFIX="%{$reset_color%}"
ZSH_THEME_GIT_PROMPT_CLEAN="%{$fg[green]%} ✔"
ZSH_THEME_GIT_PROMPT_DIRTY="%{$fg[red]%} ✗"
