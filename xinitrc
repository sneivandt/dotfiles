#!/usr/bin/env bash

if [[ -d /etc/X11/xinit/xinitrc.d ]]
then
  for f in /etc/X11/xinit/xinitrc.d/*;
  do
    [[ -x "$f" ]] && . "$f"
  done
  unset f
fi

if [[ -x $(which i3 2>/dev/null) ]]
then
  [[ -f ~/.Xresources ]] && xrdb ~/.Xresources
  xset s off
  xset -dpms
  xset s noblank
  exec i3
fi
