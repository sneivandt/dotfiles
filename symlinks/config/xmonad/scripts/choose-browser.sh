#!/bin/sh
set -o errexit
set -o nounset

for browser in chromium-dev chromium
do
  if command -v "$browser" >/dev/null 2>&1
  then
    if [ -z "${1:-}" ]
    then
      "$browser" --enable-features=OverlayScrollbar
    else
      case $(echo "$@" | tr '[:upper:]' '[:lower:]') in
        "prime video")
          "$browser" --enable-features=OverlayScrollbar --app="https://amazon.com/video"
          ;;
        "chatgpt")
          "$browser" --enable-features=OverlayScrollbar --app="https://chat.openai.com"
          ;;
        "lichess")
          "$browser" --enable-features=OverlayScrollbar --app="https://lichess.org"
          ;;
        "netflix")
          "$browser" --enable-features=OverlayScrollbar --app="https://netflix.com"
          ;;
        "youtube")
          "$browser" --enable-features=OverlayScrollbar --app="http://youtube.com/"
          ;;
        "<iframe "*)
          browser_url=$(echo "$*" | sed -n 's/.* src="\([^" '\'' ]*\)".*/\1/p')
          "$browser" --enable-features=OverlayScrollbar --app="$browser_url"
          ;;
        "file://"*)
          "$browser" --enable-features=OverlayScrollbar --app="$*"
          ;;
        "https://"*)
          "$browser" --enable-features=OverlayScrollbar --app="$*"
          ;;
        *)
          "$browser" --enable-features=OverlayScrollbar --app="https://$*"
          ;;
      esac
    fi
    exit
  fi
done
unset browser
