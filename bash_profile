#!/bin/sh

# Load .bashrc
[[ -f ~/.bashrc ]] && . ~/.bashrc

# Add ~/bin to $PATH
export PATH="$HOME/bin:$PATH"

# Start X Windows
[[ -z $DISPLAY && $XDG_VTNR -eq 1 ]] && exec startx
