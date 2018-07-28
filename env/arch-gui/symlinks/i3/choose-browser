#!/bin/bash

for browser in chromium-dev chromium firefox
do
  if [[ -n $(command -v $browser) ]]
  then
    exec $browser
    exit
  fi
done

i3-nagbar -m "Could not find a browser."
