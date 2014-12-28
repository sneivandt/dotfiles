#!/bin/sh

# Load .bashrc
[[ -f ~/.bashrc ]] && . ~/.bashrc

# Yay Vim!
export EDITOR=vim

# Add ~/bin to $PATH
export PATH="$PATH:$HOME/bin"

# Start X Windows
[[ -z $DISPLAY && $XDG_VTNR -eq 1 ]] && exec startx
