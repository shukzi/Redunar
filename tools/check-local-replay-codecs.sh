#!/usr/bin/env bash
set -euo pipefail

replay_dir=${1:-"${HOME}/Videos/Redunar Replays"}
for command in ffprobe ffmpeg gst-launch-1.0 mpv; do
  command -v "$command" >/dev/null || {
    printf 'Required local decoder is missing: %s\n' "$command" >&2
    exit 1
  }
done

shopt -s nullglob
clips=("$replay_dir"/*.mkv "$replay_dir"/*.mp4)
if ((${#clips[@]} == 0)); then
  printf 'No Redunar clips found in %s\n' "$replay_dir" >&2
  exit 1
fi

for clip in "${clips[@]}"; do
  printf 'Checking %s\n' "${clip##*/}"
  ffprobe -v error -select_streams v:0 \
    -show_entries stream=codec_name,width,height \
    -of compact=p=0:nk=1 "$clip"
  duration=$(ffprobe -v error -show_entries format=duration \
    -of default=noprint_wrappers=1:nokey=1 "$clip")
  [[ "$duration" =~ ^[0-9]+([.][0-9]+)?$ ]] || {
    printf 'Could not read a finite duration for %s\n' "$clip" >&2
    exit 1
  }
  positions=(0)
  for ratio in 0.57 0.90; do
    positions+=("$(awk -v duration="$duration" -v ratio="$ratio" \
      'BEGIN { printf "%.6f", duration * ratio }')")
  done
  for position in "${positions[@]}"; do
    ffmpeg -v error -ss "$position" -i "$clip" -frames:v 1 -f null - >/dev/null
  done
  gst-launch-1.0 -q filesrc "location=$clip" '!' decodebin '!' fakesink
  mpv --no-config --really-quiet --ao=null --vo=null --frames=1 "$clip"
done

printf 'Local Redunar codec checks passed for %s clip(s).\n' "${#clips[@]}"
