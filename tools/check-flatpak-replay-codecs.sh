#!/usr/bin/env bash
set -euo pipefail

replay_dir=${1:-"${HOME}/Videos/Redunar Replays"}
runtimes=(25.08 26.08)

command -v flatpak >/dev/null || {
  printf '%s\n' 'Flatpak is required for the independent runtime codec check.' >&2
  exit 1
}
[[ -d "$replay_dir" ]] || {
  printf 'Replay directory does not exist: %s\n' "$replay_dir" >&2
  exit 1
}

for branch in "${runtimes[@]}"; do
  runtime="org.freedesktop.Platform//$branch"
  flatpak info "$runtime" >/dev/null 2>&1 || {
    printf 'Required Flatpak runtime is not installed: %s\n' "$runtime" >&2
    exit 1
  }
  printf 'Checking Redunar clips in Freedesktop runtime %s\n' "$branch"
  flatpak run \
    --filesystem="$replay_dir:ro" \
    --env=REDUNAR_REPLAY_DIR="$replay_dir" \
    --command=sh \
    "$runtime" -c '
      set -eu
      for command in ffprobe ffmpeg gst-launch-1.0 awk; do
        command -v "$command" >/dev/null || {
          printf "Runtime decoder is missing: %s\n" "$command" >&2
          exit 1
        }
      done
      count=0
      for clip in "$REDUNAR_REPLAY_DIR"/*.mkv "$REDUNAR_REPLAY_DIR"/*.mp4; do
        test -f "$clip" || continue
        count=$((count + 1))
        printf "Checking %s\n" "${clip##*/}"
        ffprobe -v error -select_streams v:0 \
          -show_entries stream=codec_name,width,height \
          -of compact=p=0:nk=1 "$clip"
        duration=$(ffprobe -v error -show_entries format=duration \
          -of default=noprint_wrappers=1:nokey=1 "$clip")
        case "$duration" in
          ""|*[!0-9.]*)
            printf "Could not read a finite duration for %s\n" "$clip" >&2
            exit 1
            ;;
        esac
        for ratio in 0 0.57 0.90; do
          position=$(awk -v duration="$duration" -v ratio="$ratio" \
            "BEGIN { printf \"%.6f\", duration * ratio }")
          ffmpeg -v error -ss "$position" -i "$clip" \
            -frames:v 1 -f null - >/dev/null
        done
        gst-launch-1.0 -q filesrc "location=$clip" "!" decodebin "!" fakesink
      done
      test "$count" -gt 0
      printf "Runtime codec check passed for %s clip(s).\n" "$count"
    '
done

printf '%s\n' 'Independent Freedesktop runtime codec checks passed.'
