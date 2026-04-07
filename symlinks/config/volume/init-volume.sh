#!/bin/sh
set -o errexit
set -o nounset

# Wait for PulseAudio to be ready and a default sink to appear.
# PipeWire restores saved state asynchronously after reporting active,
# so we must wait for the volume to stabilize before overriding it.
if command -v pactl >/dev/null 2>&1; then
  # Wait for a default sink to exist
  attempts=0
  while [ "$attempts" -lt 30 ]; do
    if pactl get-sink-volume @DEFAULT_SINK@ >/dev/null 2>&1; then
      break
    fi
    sleep 1
    attempts=$((attempts + 1))
  done

  # Wait for volume to stabilize (PipeWire state restoration to finish).
  # Poll twice with a gap; if the volume didn't change, restoration is done.
  prev=""
  stable=0
  attempts=0
  while [ "$stable" -lt 2 ] && [ "$attempts" -lt 20 ]; do
    curr=$(pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null) || curr=""
    if [ "$curr" = "$prev" ] && [ -n "$curr" ]; then
      stable=$((stable + 1))
    else
      stable=0
    fi
    prev="$curr"
    sleep 1
    attempts=$((attempts + 1))
  done

  # Override all sinks to 70%
  pactl list sinks short | awk '{print $1}' | while read -r sink; do
    pactl set-sink-mute "$sink" 0 2>/dev/null || true
    pactl set-sink-volume "$sink" 70% 2>/dev/null || true
  done
fi

# Mute capture devices by default for privacy
amixer -q sset Capture nocap 2>/dev/null || true
