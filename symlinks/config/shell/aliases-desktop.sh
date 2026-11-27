#!/bin/sh

if command -v code-insiders >/dev/null 2>&1; then
  if [ -x /usr/bin/code-insiders ]; then
    alias code="code-insiders"
  elif _code_insiders=$(command -v code-insiders 2>/dev/null); then
    if [ "$_code_insiders" != "${HOME:-}/.local/bin/code-insiders" ]; then
      alias code="code-insiders"
    elif [ -f "$_code_insiders" ]; then
      _code_insiders_helper="${XDG_CONFIG_HOME:-${HOME:-}/.config}/shell/code-insiders.sh"
      [ -r "$_code_insiders_helper" ] || _code_insiders_helper="${HOME:-}/.config/shell/code-insiders.sh"
      if [ -r "$_code_insiders_helper" ]; then
        # shellcheck source=symlinks/config/shell/code-insiders.sh
        . "$_code_insiders_helper"
        if ! _dotfiles_is_managed_code_insiders_shim "$_code_insiders"; then
          alias code="code-insiders"
        fi
        unset -f _dotfiles_is_managed_code_insiders_shim
      else
        alias code="code-insiders"
      fi
    fi
  fi
fi
unset _code_insiders _code_insiders_helper
