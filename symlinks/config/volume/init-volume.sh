#!/bin/sh
set -o errexit
set -o nounset

# Leave output volume and mute state to the audio server's saved settings.

# Mute capture devices by default for privacy. ALSA is optional, but a present
# amixer must complete successfully.
if command -v amixer >/dev/null 2>&1; then
  amixer -q sset Capture nocap
fi
