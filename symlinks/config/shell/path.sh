#!/bin/sh

_path_prepend() {
  # Move existing entries too, so inherited PATH order cannot override ours.
  # Parse without word splitting or globbing; preserve other entries literally.
  _path_remaining=${PATH:+$PATH:}
  _path_retained=
  while [ -n "$_path_remaining" ]; do
    _path_entry=${_path_remaining%%:*}
    _path_remaining=${_path_remaining#*:}
    if [ "$_path_entry" != "$1" ]; then
      _path_retained="$_path_retained:$_path_entry"
    fi
  done
  PATH="$1$_path_retained"
}

_path_prepend "${CARGO_HOME:-$HOME/.cargo}/bin"
_path_go_workspace=${GOPATH:-$HOME/src/go}
_path_prepend "${GOBIN:-${_path_go_workspace%%:*}/bin}"
_path_prepend "$HOME/.bin"
_path_prepend "$HOME/.local/bin"
export PATH

unset -f _path_prepend
unset _path_remaining _path_retained _path_entry _path_go_workspace
