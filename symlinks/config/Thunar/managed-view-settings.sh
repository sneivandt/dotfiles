#!/bin/sh

show_hidden=true
default_view=ThunarCompactView
side_pane=THUNAR_SIDEPANE_TYPE_HIDDEN_SHORTCUTS

case "${1:-}" in
  check)
    properties=$(xfconf-query --channel thunar --list --verbose) || exit 2
    hidden=''
    view=''
    pane=''
    while read -r property value; do
      case "$property" in
        /last-show-hidden) hidden=$value ;;
        /default-view) view=$value ;;
        /last-side-pane) pane=$value ;;
      esac
    done <<EOF
$properties
EOF
    [ "$hidden" = "$show_hidden" ] && [ "$view" = "$default_view" ] && [ "$pane" = "$side_pane" ]
    ;;
  apply)
    xfconf-query --channel thunar --property /last-show-hidden --create --type bool --set "$show_hidden" &&
      xfconf-query --channel thunar --property /default-view --create --type string --set "$default_view" &&
      xfconf-query --channel thunar --property /last-side-pane --create --type string --set "$side_pane"
    ;;
  *)
    echo "Expected check or apply" >&2
    exit 2
    ;;
esac
