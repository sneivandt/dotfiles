#!/bin/sh

alias ..="cd .."
alias ...="cd ../.."
alias ....="cd ../../.."
alias .....="cd ../../../../"
alias ......="cd ../../../../../../"

alias cls="clear"

alias df="df -h -T"

alias grep="grep --color=auto"

alias l="ls -h --color=auto"
alias ls="ls -h --color=auto"
alias la="ls -A"
alias ll="ls -l"

alias mkdir="mkdir -p"

alias path='echo $PATH | tr -s ":" "\n"'

alias pwsh="pwsh -nologo"

alias sudo="sudo "

alias diff="diff --color=auto"
alias ip="ip -c"

alias tmux="tmux -2 -f ~/.config/tmux/tmux.conf"

# Modern replacements
if [ -n "$(command -v eza)" ]; then
  alias l="eza"
  alias ls="eza"
  alias ll="eza -l"
  alias la="eza -la"
fi

if [ -n "$(command -v bat)" ]; then
  alias cat="bat"
fi

if [ -n "$(command -vp nvim)" ]
then
  alias vi="nvim"
elif [ -n "$(command -vp vim)" ]
then
  alias vi="vim"
fi
