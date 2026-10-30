#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
probe="$workspace_root/target/release/examples/capture_probe"
examples="$workspace_root/target/release/examples"
acceptance_root=$(mktemp -d "${TMPDIR:-/tmp}/redunar-opengl-replay-production.XXXXXX")
retained_states=()

cleanup() {
  for state in "${retained_states[@]}"; do
    rm -rf -- "$state"
  done
  rm -rf -- "$acceptance_root"
}
trap cleanup EXIT

cd -- "$workspace_root"
cargo build --release --offline \
  -p redunar-capture-opengl --lib --examples \
  -p redunar-daemon --example capture_probe \
  -p redunar-platform --bin redunar-steam-launch

run_probe() {
  local name=$1
  local target=$2
  local steam_bridge=$3
  local overlay=$4
  local fps=$5
  local width=${6:-640}
  local height=${7:-360}
  local frames=${8:-180}
  local final_width=${9:-$width}
  local final_height=${10:-$height}
  local context_replacements=${11:-0}
  local failure_stage=${12:-}
  local auxiliary_window=${13:-0}
  local resize_frame=$((frames / 2))
  local log="$acceptance_root/$name.log"

  REDUNAR_CAPTURE_PROBE_OVERLAY="$overlay" \
  REDUNAR_CAPTURE_PROBE_PRESET=compact \
  REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE=1 \
  REDUNAR_CAPTURE_PROBE_REPLAY_FPS="$fps" \
  REDUNAR_CAPTURE_PROBE_KEEP_STATE=1 \
  REDUNAR_CAPTURE_PROBE_FRAMES="$frames" \
  REDUNAR_CAPTURE_PROBE_WIDTH="$width" \
  REDUNAR_CAPTURE_PROBE_HEIGHT="$height" \
  REDUNAR_CAPTURE_PROBE_RESIZE_WIDTH="$final_width" \
  REDUNAR_CAPTURE_PROBE_RESIZE_HEIGHT="$final_height" \
  REDUNAR_CAPTURE_PROBE_RESIZE_FRAME="$resize_frame" \
  REDUNAR_CAPTURE_PROBE_CONTEXT_REPLACEMENTS="$context_replacements" \
  REDUNAR_CAPTURE_PROBE_GL_FAIL="$failure_stage" \
  REDUNAR_CAPTURE_PROBE_AUXILIARY_WINDOW="$auxiliary_window" \
  REDUNAR_CAPTURE_PROBE_STEAM_BRIDGE="$steam_bridge" \
  REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS=30 \
    "$probe" "$examples/$target" >"$log" 2>&1

  grep -Fxq 'phase=Completed' "$log"
  grep -Fxq 'capture_api=Some(OpenGl)' "$log"
  grep -Fxq 'drops=0' "$log"
  grep -Fxq 'rejects=0' "$log"
  if [[ $overlay == 1 ]]; then
    grep -Fxq 'overlay_status=Some(Active)' "$log"
  fi

  local clip
  clip=$(sed -n 's/^replay_encoded_clip=//p' "$log")
  [[ -n $clip && -s $clip ]]
  local exported_frames
  local presented_frames
  local presented_fps
  exported_frames=$(sed -n 's/^replay_exported_frames=//p' "$log")
  presented_frames=$(sed -n 's/^frames=//p' "$log")
  presented_fps=$(sed -n 's/^average_fps=//p' "$log")
  [[ $exported_frames =~ ^[1-9][0-9]*$ && $presented_frames =~ ^[1-9][0-9]*$ ]]
  [[ $presented_fps =~ ^[1-9][0-9]*(\.[0-9]+)?$ ]]
  # Compare against the active presentation window, excluding launcher and
  # wrapper startup. Capture must deliver at least 85% of the requested rate;
  # when the game is slower than that rate, one export per present is the cap.
  awk -v exported="$exported_frames" -v presented="$presented_frames" \
    -v game_fps="$presented_fps" -v replay_fps="$fps" 'BEGIN {
      expected = presented * replay_fps / game_fps
      if (expected > presented) expected = presented
      exit !(exported >= expected * 0.85)
    }'
  grep -Fxq "replay_candidate_width=$final_width" "$log"
  grep -Fxq "replay_candidate_height=$final_height" "$log"
  if ((context_replacements > 0)); then
    grep -Fxq "GLX probe context replacements=$context_replacements" "$log"
  fi
  if [[ $auxiliary_window == 1 ]]; then
    grep -Fxq 'GLX probe auxiliary window=true' "$log"
    # The helper swaps once per main frame, but only the selected game window
    # may contribute metrics or Replay cadence.
    ((presented_frames == frames - 1))
  fi
  if [[ -n $failure_stage ]]; then
    grep -Fxq "Redunar OpenGL Replay injected failure: $failure_stage" "$log"
  fi
  local state
  state=$(sed -n 's/^Redunar capture probe retained state at //p' "$log")
  [[ -n $state ]]
  retained_states+=("$state")

  ffprobe -v error -select_streams v:0 \
    -count_frames \
    -show_entries stream=codec_name,width,height,avg_frame_rate,pix_fmt,nb_read_frames \
    -of default=noprint_wrappers=1 "$clip" >"$acceptance_root/$name.ffprobe"
  grep -Fxq 'codec_name=h264' "$acceptance_root/$name.ffprobe"
  grep -Fxq "width=$final_width" "$acceptance_root/$name.ffprobe"
  grep -Fxq "height=$final_height" "$acceptance_root/$name.ffprobe"
  grep -Fxq 'pix_fmt=yuv420p' "$acceptance_root/$name.ffprobe"
  grep -Fxq "avg_frame_rate=$fps/1" "$acceptance_root/$name.ffprobe"
  local encoded_frames
  encoded_frames=$(sed -n 's/^nb_read_frames=//p' "$acceptance_root/$name.ffprobe")
  [[ $encoded_frames =~ ^[1-9][0-9]*$ ]]
  if [[ $final_width == "$width" && $final_height == "$height" ]]; then
    [[ $encoded_frames == "$exported_frames" ]]
  else
    # A dimension change starts a fresh codec epoch and intentionally discards
    # the old-size history. Validate cadence only across the post-resize half.
    awk -v encoded="$encoded_frames" -v presented="$presented_frames" \
      -v resize_frame="$resize_frame" -v game_fps="$presented_fps" \
      -v replay_fps="$fps" 'BEGIN {
        remaining = presented - resize_frame
        expected = remaining * replay_fps / game_fps
        if (expected > remaining) expected = remaining
        exit !(encoded >= expected * 0.85)
      }'
  fi

  printf '%-26s ' "$name"
  awk -F= '/^(frames|drops|rejects|replay_encoded_bytes)=/{printf "%s=%s ", $1, $2} END{print ""}' "$log"
}

run_probe glx-direct-overlay-60 glx_probe 0 1 60
run_probe glx-steam-hidden-30 glx_probe 1 0 30
run_probe sdl-direct-hidden-60 sdl_gl_probe 0 0 60
run_probe sdl-steam-overlay-30 sdl_gl_probe 1 1 30
run_probe glx-1080p-overlay-60 glx_probe 0 1 60 1920 1080 600
run_probe glx-1080p-overlay-120 glx_probe 0 1 120 1920 1080 600
run_probe glx-resize-overlay-60 glx_probe 0 1 60 640 360 600 1280 720
run_probe glx-context-replace-60 glx_probe 0 1 60 1280 720 600 1280 720 6
run_probe glx-two-windows-60 glx_probe 0 1 60 640 360 600 640 360 0 '' 1
run_probe glx-fail-allocate glx_probe 0 1 60 640 360 180 640 360 0 allocate
run_probe glx-fail-import glx_probe 0 1 60 640 360 180 640 360 0 import
run_probe glx-fail-fence glx_probe 0 1 60 640 360 180 640 360 0 fence
run_probe glx-fail-transfer glx_probe 0 1 60 640 360 180 640 360 0 transfer

saturation_log="$acceptance_root/glx-held-releases.log"
REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE=1 \
REDUNAR_CAPTURE_PROBE_HOLD_REPLAY_RELEASES=1 \
REDUNAR_CAPTURE_PROBE_OVERLAY=1 \
REDUNAR_CAPTURE_PROBE_FRAMES=600 \
REDUNAR_CAPTURE_PROBE_WIDTH=640 \
REDUNAR_CAPTURE_PROBE_HEIGHT=360 \
REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS=20 \
  "$probe" "$examples/glx_probe" >"$saturation_log" 2>&1
grep -Fxq 'phase=Completed' "$saturation_log"
grep -Fxq 'held_replay_exports=6' "$saturation_log"
grep -Fxq 'frames=599' "$saturation_log"
grep -Fxq 'drops=0' "$saturation_log"
grep -Fxq 'rejects=0' "$saturation_log"
pool_drops=$(sed -n 's/.* drops=\([0-9][0-9]*\)$/\1/p' "$saturation_log")
[[ $pool_drops =~ ^[0-9]+$ ]]
((pool_drops > 0))
printf 'glx-held-releases           producer_replay_drops=%s\n' "$pool_drops"

printf '%s\n' 'OpenGL production Replay acceptance passed.'
