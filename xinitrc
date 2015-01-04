#!/usr/bin/env bash

# Run system scripts
if [[ -d /etc/X11/xinit/xinitrc.d ]]
then
  for f in /etc/X11/xinit/xinitrc.d/*;
  do
    [[ -x "$f" ]] && . "$f"
  done
  unset f
fi

# Do nothing if i3 is not executable
[[ ! -x $(which i3 2>/dev/null) ]] && exit

# Load X resources
[[ -f ~/.Xresources ]] && xrdb ~/.Xresources

# Disable screen blanking
xset s off
xset -dpms
xset s noblank

# Launch i3
exec i3
