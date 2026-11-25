#!/bin/sh
set -eu

export LC_ALL=C
node=@DEFAULT_AUDIO_SINK@
label=Volume
kind=volume
icon=audio-volume-high-symbolic
tag=dotfiles-volume
case "${1:-}" in
  up) wpctl set-volume --limit 1.5 "$node" 5%+ ;;
  down) wpctl set-volume --limit 1.5 "$node" 5%- ;;
  mute) wpctl set-mute "$node" toggle ;;
  mic-mute)
    node=@DEFAULT_AUDIO_SOURCE@
    label=Microphone
    kind=microphone
    icon=audio-input-microphone-symbolic
    tag=dotfiles-microphone
    wpctl set-mute "$node" toggle
    ;;
  *) printf 'Usage: %s up|down|mute|mic-mute\n' "$0" >&2; exit 2 ;;
esac

# Feedback is optional; an OSD or notification failure must not undo an audio action.
state=$(wpctl get-volume "$node") || exit 0
percent=$(printf '%s\n' "$state" | awk '{ printf "%.0f", $2 * 100 }')
case "$state" in
  *'[MUTED]'*)
    muted=true
    summary="$label muted"
    ;;
  *)
    muted=false
    summary="$label $percent%"
    ;;
esac

if command -v quickshell >/dev/null 2>&1 \
  && quickshell ipc call osd display "$kind" "$percent" "$muted" >/dev/null 2>&1; then
  exit 0
fi

command -v notify-send >/dev/null 2>&1 || exit 0
notify-send --app-name=dotfiles --category=progress --expire-time=2000 \
  --hint="string:x-canonical-private-synchronous:$tag" \
  --hint="int:value:$percent" --icon="$icon" "$summary" || true
