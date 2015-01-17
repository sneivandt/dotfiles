#!/usr/bin/env bash

# Load .bashrc
[[ -f ~/.bashrc ]] && . ~/.bashrc

# Yay Vim!
export EDITOR=vim

# Less
export LESS=-imR

# Add ~/bin to $PATH
export PATH="$PATH:$HOME/bin"

# Start X Windows
[[ -z $DISPLAY && $XDG_VTNR -eq 1 && -x $(which i3 2>/dev/null) ]] && exec startx
