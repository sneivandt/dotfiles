#!/bin/sh

alias ..="cd .."
alias ...="cd ../.."
alias ....="cd ../../.."
alias .....="cd ../../../../"
alias ......="cd ../../../../../../"

alias cls="clear"

alias df="df -h -T"

alias grep="grep -i --color=auto"

alias l="ls"
alias ls="ls -h --color=auto"
alias la="ls -A"
alias ll="ls -l"

alias mkdir="mkdir -p"

alias path='echo $PATH | tr -s ":" "\n"'

alias pwsh="pwsh -nologo"

alias sudo="sudo "

alias tmux="tmux -2"

if [ -n "$(command -vp nvim)" ]
then
  alias vi="nvim"
elif [ -n "$(command -vp vim)" ]
then
  alias vi="vim"
fi
