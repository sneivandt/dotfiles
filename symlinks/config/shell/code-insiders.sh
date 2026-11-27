#!/bin/sh
# Shared dotfiles-managed code-insiders shim detection for shell aliases and launchers.
# Keep POSIX-compatible and safe under `set -u` without altering caller options.

# Returns 0 when $1 is the current or legacy dotfiles-managed code-insiders shim.
_dotfiles_is_managed_code_insiders_shim() {
  _dmcis_path="$1"
  [ -f "$_dmcis_path" ] || {
    unset _dmcis_path
    return 1
  }
  if { IFS= read -r _dmcis_line && IFS= read -r _dmcis_line; } < "$_dmcis_path" 2>/dev/null &&
     [ "$_dmcis_line" = "# dotfiles-managed-shim: code-insiders" ]; then
    unset _dmcis_path _dmcis_line
    return 0
  fi
  if _dmcis_digest=$(sha256sum 2>/dev/null < "$_dmcis_path"); then
    case "${_dmcis_digest%% *}" in
      d07a47ed11a9b15b283df8d89264df3539749273178898db2efc3f42485fe332 | \
      abbafb0e03fa9c8534a5a74a46cbfc4357e73a34497da0966f96cddd6017eec9)
        unset _dmcis_path _dmcis_line _dmcis_digest
        return 0
        ;;
    esac
  fi
  unset _dmcis_path _dmcis_line _dmcis_digest
  return 1
}
