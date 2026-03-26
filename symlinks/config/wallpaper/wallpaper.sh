#!/bin/sh
set -o errexit
set -o nounset

feh --bg-fill --no-fehbg "${XDG_CONFIG_HOME:-$HOME/.config}"/wallpaper/default.png
