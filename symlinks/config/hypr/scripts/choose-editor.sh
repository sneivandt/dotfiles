#!/bin/sh
set -o errexit
set -o nounset

helper="${XDG_CONFIG_HOME:-${HOME:-}/.config}/shell/code-insiders.sh"
if [ ! -r "$helper" ]; then
  helper="${HOME:-}/.config/shell/code-insiders.sh"
fi
if [ ! -r "$helper" ] && [ -f "$0" ]; then
  helper="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)/../../shell/code-insiders.sh"
fi
has_shim_helper=0
if [ -r "$helper" ]; then
  # shellcheck source=symlinks/config/shell/code-insiders.sh
  . "$helper"
  has_shim_helper=1
fi

for editor in code-insiders code gvim
do
  if launcher=$(command -v "$editor" 2>/dev/null)
  then
    # Removal can leave a current or legacy managed shim as a regular file,
    # even without the checkout. Do not mistake a custom launcher at that path
    # for it.
    if [ "$editor" = code-insiders ] &&
       [ "$launcher" = "${HOME:-}/.local/bin/code-insiders" ] &&
       [ ! -x /usr/bin/code-insiders ] &&
       [ "$has_shim_helper" -eq 1 ] &&
       _dotfiles_is_managed_code_insiders_shim "$launcher"; then
      continue
    fi
    exec "$editor" "$@"
  fi
done
unset editor helper has_shim_helper

echo "ERROR: No supported editor found" >&2
exit 1
