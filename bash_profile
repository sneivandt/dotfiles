#!/bin/sh

# Load .bashrc
[[ -f ~/.bashrc ]] && . ~/.bashrc

# Start X Windows
[[ -z $DISPLAY && $XDG_VTNR -eq 1 ]] && exec startx
