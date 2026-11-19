#!/bin/sh
set -o errexit
set -o nounset

for editor in code-insiders code gvim
do
  if launcher=$(command -v "$editor" 2>/dev/null)
  then
    # Removal can leave the managed shim as a regular file, even without
    # the checkout. Do not mistake a custom launcher at that path for it.
    if [ "$editor" = code-insiders ] &&
       [ "$launcher" = "${HOME:-}/.local/bin/code-insiders" ] &&
       [ ! -x /usr/bin/code-insiders ] &&
       [ -f "$launcher" ] &&
       digest=$(sha256sum 2>/dev/null < "$launcher") &&
       [ "${digest%% *}" = abbafb0e03fa9c8534a5a74a46cbfc4357e73a34497da0966f96cddd6017eec9 ]; then
      continue
    fi
    exec "$editor" "$@"
  fi
done
unset editor

echo "ERROR: No supported editor found" >&2
exit 1
