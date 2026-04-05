#!/bin/sh
set -o errexit
set -o nounset

for browser in chromium-dev chromium
do
  if command -v "$browser" >/dev/null 2>&1
  then
    if [ -z "${1:-}" ]
    then
      exec "$browser"
    else
      case $(echo "$@" | tr '[:upper:]' '[:lower:]') in
        "prime video")
          exec "$browser" --app="https://amazon.com/video"
          ;;
        "chatgpt")
          exec "$browser" --app="https://chat.openai.com"
          ;;
        "lichess")
          exec "$browser" --app="https://lichess.org"
          ;;
        "netflix")
          exec "$browser" --app="https://netflix.com"
          ;;
        "youtube")
          exec "$browser" --app="https://youtube.com/"
          ;;
        "https://"*)
          exec "$browser" --app="$*"
          ;;
        *)
          exec "$browser" --app="https://$*"
          ;;
      esac
    fi
  fi
done
unset browser

echo "ERROR: No supported browser found" >&2
exit 1
