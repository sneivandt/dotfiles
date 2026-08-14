#!/bin/sh
set -o errexit
set -o nounset

# Wait for PulseAudio to be ready and a default sink to appear.
# PipeWire restores saved state asynchronously after reporting active,
# so we must wait for the volume to stabilize before overriding it.
if ! command -v pactl >/dev/null 2>&1; then
  printf 'ERROR: pactl is required to initialize audio volume\n' >&2
  exit 1
fi

# Wait for a default sink to exist.
attempts=0
while [ "$attempts" -lt 30 ]; do
  if pactl get-sink-volume @DEFAULT_SINK@ >/dev/null 2>&1; then
    break
  fi
  sleep 1
  attempts=$((attempts + 1))
done

if ! pactl get-sink-volume @DEFAULT_SINK@ >/dev/null 2>&1; then
  printf 'ERROR: no default audio sink appeared after 30 seconds\n' >&2
  exit 1
fi

# Wait for volume to stabilize (PipeWire state restoration to finish).
# Poll twice with a gap; if the volume did not change, restoration is done.
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

if [ "$stable" -lt 2 ]; then
  printf 'ERROR: default audio sink volume did not stabilize after 20 seconds\n' >&2
  exit 1
fi

sink_rows=$(pactl list sinks short)
sinks=$(printf '%s\n' "$sink_rows" | awk '{print $1}')
if [ -z "$sinks" ]; then
  printf 'ERROR: pactl reported no audio sinks\n' >&2
  exit 1
fi

while IFS= read -r sink; do
  pactl set-sink-mute "$sink" 0
  pactl set-sink-volume "$sink" 70%
done <<EOF
$sinks
EOF

# Mute capture devices by default for privacy. ALSA is optional, but a present
# amixer must complete successfully.
if command -v amixer >/dev/null 2>&1; then
  amixer -q sset Capture nocap
fi
