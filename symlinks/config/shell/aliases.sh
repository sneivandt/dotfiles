#!/bin/sh

alias ..="cd .."
alias ...="cd ../.."
alias ....="cd ../../.."
alias .....="cd ../../../../"
alias ......="cd ../../../../../../"

alias cls="clear"

alias df="df -h -T"

alias g="git"

alias grep="grep --color=auto"

alias l="ls -h --color=auto"
alias ls="ls -h --color=auto"
alias la="ls -A"
alias ll="ls -l"

alias mkdir="mkdir -p"

alias path='echo "$PATH" | tr -s ":" "\n"'

alias pwsh="pwsh -nologo"

alias sudo="sudo "

alias diff="diff --color=auto"
alias ip="ip -c"

alias tmux="tmux -2 -f ~/.config/tmux/tmux.conf"

# Modern replacements
if command -v eza >/dev/null 2>&1; then
  alias l="eza"
  alias ls="eza"
  alias ll="eza -l"
  alias la="eza -la"
  alias tree="eza --tree"
elif command -v exa >/dev/null 2>&1; then
  alias l="exa"
  alias ls="exa"
  alias ll="exa -l"
  alias la="exa -la"
fi

if command -v bat >/dev/null 2>&1; then
  alias cat="bat"
elif command -v batcat >/dev/null 2>&1; then
  alias cat="batcat"
fi

if command -v nvim >/dev/null 2>&1; then
  alias vi="nvim"
elif command -v vim >/dev/null 2>&1; then
  alias vi="vim"
fi

if command -v tldr >/dev/null 2>&1; then
  alias help="tldr"
fi

if command -v xclip >/dev/null 2>&1; then
  alias clip="xclip -selection clipboard"
  alias pbcopy="xclip -selection clipboard -in"
  alias pbpaste="xclip -selection clipboard -out"
fi

# AI / GitHub Copilot CLI aliases
if command -v gh >/dev/null 2>&1 && gh copilot --version >/dev/null 2>&1; then
  # Chat mode: interactive if no args, prompt mode if args provided
  ai() {
    if [ $# -eq 0 ]; then
      gh copilot
    else
      gh copilot -p "$*"
    fi
  }
  # Suggest mode: interactive
  alias aic="gh copilot -i suggest"
fi
