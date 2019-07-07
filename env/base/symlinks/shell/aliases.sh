#!/bin/sh

#
# Change default behaviour
#

alias df="df -h -T"

alias grep="grep -i --color=auto"

alias l="ls"
alias ls="ls -h --color=auto"
alias la="ls -a"
alias ll="ls -l"

alias mkdir="mkdir -p"

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

if [ -n "$(command -vp code-insiders)" ]
then
  alias code="code-insiders"
fi

#
# Other aliases
#

# Navigation
alias ..="cd .."
alias ...="cd ../.."
alias ....="cd ../../.."
alias .....="cd ../../../../"
alias ......="cd ../../../../../../"

# Path
alias path="echo $PATH | tr -s ':' '\n'"

# PowerShell has corrupted me
alias cls="clear"
