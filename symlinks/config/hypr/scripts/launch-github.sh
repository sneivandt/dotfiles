#!/bin/sh
set -o errexit
set -o nounset

app=/usr/bin/github

if [ ! -x "$app" ]; then
  logger -t launch-github "GitHub Copilot is not installed at $app"
  exit 1
fi

export GDK_BACKEND=x11
export GTK_CSD=0

has_window()
{
  hyprctl -j clients 2>/dev/null |
    jq -e 'any(.[];
      ((.class // "") | ascii_downcase) == "github" or
      ((.initialClass // "") | ascii_downcase) == "github"
    )' \
      >/dev/null
}

if command -v hyprctl >/dev/null 2>&1 &&
   command -v jq >/dev/null 2>&1 &&
   has_window
then
  exec "$app"
fi

# A plain launch can leave v1.1.10 running tray-only. This supported deep link
# creates or restores the main window without restarting background sessions.
exec "$app" "ghapp://mywork"
