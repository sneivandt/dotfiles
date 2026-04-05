#!/bin/sh
set -o errexit
set -o nounset

for editor in code-insiders code gvim
do
  if command -v "$editor" >/dev/null 2>&1
  then
    exec "$editor"
  fi
done
unset editor

echo "ERROR: No supported editor found" >&2
exit 1
