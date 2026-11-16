#!/bin/sh

show_hidden=true
default_view=ThunarCompactView

case "${1:-}" in
  check)
    properties=$(xfconf-query --channel thunar --list --verbose) || exit 2
    hidden=''
    view=''
    while read -r property value; do
      case "$property" in
        /last-show-hidden) hidden=$value ;;
        /default-view) view=$value ;;
      esac
    done <<EOF
$properties
EOF
    [ "$hidden" = "$show_hidden" ] && [ "$view" = "$default_view" ]
    ;;
  apply)
    xfconf-query --channel thunar --property /last-show-hidden --create --type bool --set "$show_hidden" &&
      xfconf-query --channel thunar --property /default-view --create --type string --set "$default_view"
    ;;
  *)
    echo "Expected check or apply" >&2
    exit 2
    ;;
esac
