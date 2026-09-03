#!/bin/sh

alias ..="cd .."
alias ...="cd ../.."
alias ....="cd ../../.."
alias .....="cd ../../../.."
alias ......="cd ../../../../.."

alias cls="clear"

alias df="df -h -T"

alias dot="dotfiles"

alias g="git"

alias grep="grep --color=auto"

alias l="ls -h --color=auto"
alias ls="ls -h --color=auto"
alias la="ls -A"
alias ll="ls -l"

alias md="mkdir -p"

alias path='echo "$PATH" | tr -s ":" "\n"'

alias pwsh="pwsh -nologo"

alias sudo="sudo "

alias diff="diff --color=auto"
alias ip="ip -c"

if command -v pacman >/dev/null 2>&1 && [ -r "$HOME/.config/pacman.conf" ]; then
  alias pacman='pacman --config "$HOME/.config/pacman.conf"'
fi

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

if ! command -v bat >/dev/null 2>&1 && command -v batcat >/dev/null 2>&1; then
  alias bat="batcat"
fi

if command -v nvim >/dev/null 2>&1; then
  alias vi="nvim"
elif command -v vim >/dev/null 2>&1; then
  alias vi="vim"
fi

if command -v tldr >/dev/null 2>&1; then
  # Deliberately not "help": that would shadow the bash builtin.
  alias tl="tldr"
fi

if command -v xclip >/dev/null 2>&1; then
  alias clip="xclip -selection clipboard"
  alias pbcopy="xclip -selection clipboard -in"
  alias pbpaste="xclip -selection clipboard -out"
fi

# AI CLI aliases
if command -v codex >/dev/null 2>&1; then
  alias ai="codex --approve-for-me"
fi
