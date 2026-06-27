#!/bin/sh

_path_prepend() {
  case ":$PATH:" in
    *":$1:"*) ;;
    *) PATH="$1${PATH:+:$PATH}" ;;
  esac
}

_path_prepend "$HOME/.cargo/bin"
_path_prepend "$HOME/src/go/bin"
_path_prepend "$HOME/.bin"
_path_prepend "$HOME/.local/bin"
export PATH

unset -f _path_prepend
