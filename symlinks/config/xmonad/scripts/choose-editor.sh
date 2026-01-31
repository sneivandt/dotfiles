#!/bin/sh
set -o errexit
set -o nounset

for editor in code-insiders code gvim
do
  if command -v "$editor" >/dev/null 2>&1
  then
    $editor
    exit
  fi
done
unset editor
