#!/bin/sh
set -o errexit
set -o nounset

for editor in code-insiders code gvim
do
  if launcher=$(command -v "$editor" 2>/dev/null)
  then
    # The managed shim remains on PATH if its package is removed.
    if [ "$editor" = code-insiders ] &&
       [ "$launcher" = "${HOME:-}/.local/bin/code-insiders" ] &&
       [ ! -x /usr/bin/code-insiders ]; then
      continue
    fi
    exec "$editor" "$@"
  fi
done
unset editor

echo "ERROR: No supported editor found" >&2
exit 1
