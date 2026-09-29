#!/bin/sh
set -o errexit
set -o nounset

for browser in chromium-dev chromium
do
  if command -v "$browser" >/dev/null 2>&1
  then
    normalized=$(printf '%s' "$*" | tr '[:upper:]' '[:lower:]')

    case "$normalized" in
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
    esac

    if [ "$#" -ne 1 ]; then
      exec "$browser" "$@"
    fi

    target=$1
    case "$target" in
      http://*|https://*)
        exec "$browser" --app="$target"
        ;;
      *:*)
        exec "$browser" "$target"
        ;;
      /*|./*|../*|~/*|-*)
        exec "$browser" "$target"
        ;;
      *.*)
        exec "$browser" --app="https://$target"
        ;;
      *)
        exec "$browser" "$target"
        ;;
    esac
  fi
done
unset browser

echo "ERROR: No supported browser found" >&2
exit 1
