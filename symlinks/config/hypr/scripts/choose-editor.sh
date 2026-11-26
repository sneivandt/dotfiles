#!/bin/sh
set -o errexit
set -o nounset

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
       [ -f "$launcher" ]; then
      if { IFS= read -r line && IFS= read -r line; } < "$launcher" 2>/dev/null &&
         [ "$line" = "# dotfiles-managed-shim: code-insiders" ]; then
        continue
      fi
      if digest=$(sha256sum 2>/dev/null < "$launcher"); then
        case "${digest%% *}" in
          d07a47ed11a9b15b283df8d89264df3539749273178898db2efc3f42485fe332 | \
          abbafb0e03fa9c8534a5a74a46cbfc4357e73a34497da0966f96cddd6017eec9)
            continue
            ;;
        esac
      fi
    fi
    exec "$editor" "$@"
  fi
done
unset editor

echo "ERROR: No supported editor found" >&2
exit 1
