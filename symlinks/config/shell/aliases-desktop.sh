#!/bin/sh

if command -v code-insiders >/dev/null 2>&1; then
  if [ -x /usr/bin/code-insiders ]; then
    alias code="code-insiders"
  elif _code_insiders=$(command -v code-insiders 2>/dev/null); then
    if [ "$_code_insiders" != "${HOME:-}/.local/bin/code-insiders" ]; then
      alias code="code-insiders"
    elif [ -f "$_code_insiders" ] &&
         _code_insiders_digest=$(sha256sum 2>/dev/null < "$_code_insiders"); then
      case "${_code_insiders_digest%% *}" in
        d07a47ed11a9b15b283df8d89264df3539749273178898db2efc3f42485fe332 | \
        abbafb0e03fa9c8534a5a74a46cbfc4357e73a34497da0966f96cddd6017eec9) ;;
        *) alias code="code-insiders" ;;
      esac
    fi
  fi
fi
unset _code_insiders _code_insiders_digest
