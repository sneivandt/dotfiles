#!/bin/sh
set -o errexit
set -o nounset

# Leave output volume and mute state to the audio server's saved settings.

# Mute the default microphone through the audio server for privacy.
exec pactl set-source-mute @DEFAULT_SOURCE@ 1
