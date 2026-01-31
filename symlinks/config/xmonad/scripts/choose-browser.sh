#!/bin/sh
set -o errexit
set -o nounset

for browser in chromium-dev chromium
do
  if command -v "$browser" >/dev/null 2>&1
  then
    browser="$browser --enable-features=OverlayScrollbar"
    if [ -z "${1:-}" ]
    then
      $browser
    else
      case $(echo "$@" | tr '[:upper:]' '[:lower:]') in
        "prime video")
          $browser --app="https://amazon.com/video"
          ;;
        "chatgpt")
          $browser --app="https://chat.openai.com"
          ;;
        "lichess")
          $browser --app="https://lichess.org"
          ;;
        "netflix")
          $browser --app="https://netflix.com"
          ;;
        "youtube")
          $browser --app="http://youtube.com/"
          ;;
        "<iframe "*)
          browser_url=$(echo "$*" | sed -n 's/.* src="\([^" '\'' ]*\)".*/\1/p')
          $browser --app="$browser_url"
          ;;
        "file://"*)
          $browser --app="$*"
          ;;
        "https://"*)
          $browser --app="$*"
          ;;
        *)
          $browser --app="https://$*"
          ;;
      esac
    fi
    exit
  fi
done
unset browser
