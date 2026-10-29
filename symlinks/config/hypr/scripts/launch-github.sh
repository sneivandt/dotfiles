#!/bin/sh
set -o errexit
set -o nounset

app=/usr/bin/github

report()
{
  printf '%s\n' "$*" >&2
  logger -t launch-github "$*" || true
}

if [ ! -x "$app" ]; then
  report "GitHub Copilot is not installed at $app"
  exit 1
fi

export GDK_BACKEND=x11
export GTK_CSD=0

# Protocol launches must retain their original URI, including authentication links.
if [ "$#" -gt 0 ]; then
  exec "$app" "$@"
fi

if ! command -v jq >/dev/null 2>&1; then
  report "jq is unavailable; Copilot window recovery is disabled"
  exec "$app" "ghapp://mywork"
fi

reopen_from_tray()
{
  if ! registry=$(busctl --user --timeout=3 --json=short get-property \
    org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
    org.kde.StatusNotifierWatcher RegisteredStatusNotifierItems); then
    report "Could not query tray items; trying a normal Copilot launch"
    return 1
  fi
  if ! items=$(printf '%s\n' "$registry" | jq -r '.data[]'); then
    report "Invalid tray item response"
    return 2
  fi

  while IFS= read -r item; do
    [ -n "$item" ] || continue
    case "$item" in
      */*) service=${item%%/*}; path=/${item#*/} ;;
      *) service=$item; path=/StatusNotifierItem ;;
    esac
    if ! title=$(busctl --user --timeout=3 --json=short get-property \
      "$service" "$path" org.kde.StatusNotifierItem Title); then
      report "Could not inspect a tray item; it may have exited"
      continue
    fi
    if ! printf '%s\n' "$title" | jq -e '
      .data | ascii_downcase | . == "github" or . == "github copilot"
    ' >/dev/null; then
      continue
    fi

    # Linux Copilot restores its window through the tray menu, not Activate or
    # a second-instance deep link. Menu IDs change as sessions are added.
    if ! menu=$(busctl --user --timeout=3 --json=short get-property \
      "$service" "$path" org.kde.StatusNotifierItem Menu) ||
       ! menu_path=$(printf '%s\n' "$menu" | jq -er '.data') ||
       ! layout=$(busctl --user --timeout=3 --json=short call -- \
         "$service" "$menu_path" com.canonical.dbusmenu GetLayout iias 0 1 1 label) ||
       ! action=$(printf '%s\n' "$layout" | jq -er '
         .data[1][2][] | .data |
         select(.[1].label.data? == "Open GitHub Copilot") | .[0]
       '); then
      report "Could not find Copilot's Open GitHub Copilot tray action"
      return 2
    fi
    if ! busctl --user --timeout=3 call "$service" "$menu_path" \
      com.canonical.dbusmenu Event isvu "$action" clicked i 0 0; then
      report "Could not activate Copilot's tray action"
      return 2
    fi
    return 0
  done <<EOF
$items
EOF
  return 1
}

if command -v busctl >/dev/null 2>&1; then
  if reopen_from_tray; then
    exit 0
  else
    status=$?
    [ "$status" -eq 1 ] || exit 1
  fi
else
  report "busctl is unavailable; Copilot tray recovery is disabled"
fi

# Retain the deep link for cold starts; it does not restore a tray-only instance.
exec "$app" "ghapp://mywork"
