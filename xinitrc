#!/bin/sh

# Load X resources
[[ -f ~/.Xresources ]] && xrdb ~/.Xresources

# Disable screen blanking
xset s off
xset -dpms
xset s noblank

# Launch i3
[[ -x $(which i3 2>/dev/null) ]] && exec i3
