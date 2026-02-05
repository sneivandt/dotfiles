#!/bin/sh
set -o errexit
set -o nounset

# Unmute and set volume for PulseAudio/PipeWire sinks first
# This needs to happen before ALSA mixer controls
if command -v pactl >/dev/null 2>&1; then
  # Wait a moment for PulseAudio to fully initialize
  sleep 2

  # Unmute all sinks and set to 70%
  pactl list sinks short | awk '{print $1}' | while read -r sink; do
    pactl set-sink-mute "$sink" 0 2>/dev/null || true
    pactl set-sink-volume "$sink" 70% 2>/dev/null || true
  done
fi

# Unmute and set Master volume
# Set Master to a reasonable default, so it acts as the main gain control.
amixer -q sset Master unmute 2>/dev/null || true
amixer -q sset Master 70% 2>/dev/null || true

# Unmute and maximize secondary output channels.
# By setting these to 100% (0dB), we ensure 'Master' dictates the actual output level.
# We ignore errors because not all sound cards have all these channels.
for channel in Headphone Speaker PCM Front Surround Center LFE
do
  amixer -q sset "$channel" unmute 2>/dev/null || true
  amixer -q sset "$channel" 100% 2>/dev/null || true
done

# Mute capture devices by default for privacy
amixer -q sset Capture nocap 2>/dev/null || true
