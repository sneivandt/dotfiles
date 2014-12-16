#!/bin/sh

# Load X resources
[[ -f ~/.Xresources ]] && xrdb ~/.Xresources

# Launch i3wm
[[ -f /usr/bin/i3 ]] && exec i3
