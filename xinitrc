#!/bin/sh

# Disable screen blanking
xset s off
xset -dpms
xset s noblank

# Load X resources
[[ -f ~/.Xresources ]] && xrdb ~/.Xresources

# Launch i3
[[ -x $(which i3 2>/dev/null) ]] && exec i3
