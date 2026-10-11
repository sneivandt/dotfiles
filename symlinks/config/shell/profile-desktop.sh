#!/bin/sh

# gtk
if [ -z "${GTK2_RC_FILES+x}" ] && [ -r "${XDG_CONFIG_HOME:-$HOME/.config}/gtk-2.0/gtkrc-2.0" ]; then
  export GTK2_RC_FILES="${XDG_CONFIG_HOME:-$HOME/.config}/gtk-2.0/gtkrc-2.0"
fi

# nvidia
if [ -z "${__GL_SHADER_DISK_CACHE_PATH+x}" ]; then
  export __GL_SHADER_DISK_CACHE_PATH="${XDG_CACHE_HOME:-$HOME/.cache}/GLCache"
fi
