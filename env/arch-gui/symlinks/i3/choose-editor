#!/bin/bash

for editor in code-insiders code gvim
do
  if [[ -n $(command -v $editor) ]]
  then
    exec $editor
    exit
  fi
done

i3-nagbar -m "Could not find an editor."
