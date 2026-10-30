#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
probe="$workspace_root/target/release/examples/capture_probe"
examples="$workspace_root/target/release/examples"
acceptance_root=$(mktemp -d "${TMPDIR:-/tmp}/redunar-opengl-replay.XXXXXX")
trap 'rm -rf -- "$acceptance_root"' EXIT

cd -- "$workspace_root"
cargo build --release --offline \
  -p redunar-capture-opengl --lib --examples \
  -p redunar-daemon --example capture_probe \
  -p redunar-platform --bin redunar-steam-launch

run_probe() {
  local name=$1
  local target=$2
  local steam_bridge=$3
  local log="$acceptance_root/$name.log"

  REDUNAR_CAPTURE_PROBE_OVERLAY=1 \
  REDUNAR_CAPTURE_PROBE_PRESET=compact \
  REDUNAR_CAPTURE_PROBE_REPLAY_CANDIDATE=1 \
  REDUNAR_CAPTURE_PROBE_REQUIRE_REPLAY_CANDIDATE=1 \
  REDUNAR_CAPTURE_PROBE_STEAM_BRIDGE="$steam_bridge" \
  REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS=15 \
    "$probe" "$examples/$target" >"$log" 2>&1

  grep -Fxq 'phase=Completed' "$log"
  grep -Fxq 'capture_api=Some(OpenGl)' "$log"
  grep -Fxq 'drops=0' "$log"
  grep -Fxq 'rejects=0' "$log"
  grep -Fxq 'overlay_status=Some(Active)' "$log"
  grep -Eq '^replay_candidate_width=[1-9][0-9]*$' "$log"
  grep -Eq '^replay_candidate_height=[1-9][0-9]*$' "$log"
  grep -Fxq 'replay_candidate_format=Rgba8Unorm' "$log"
  grep -Eq '^replay_copied_frames=[1-9][0-9]*$' "$log"
  grep -Eq '^replay_latest_copied_bytes=[1-9][0-9]*$' "$log"
  grep -Eq '^replay_latest_sample_checksum=[0-9a-f]{16}$' "$log"

  printf '%-24s ' "$name"
  awk -F= '/^(frames|drops|rejects|replay_candidate_width|replay_candidate_height|replay_copied_frames)=/{printf "%s=%s ", $1, $2} END{print ""}' "$log"
}

run_probe glx-direct glx_probe 0
run_probe egl-direct egl_probe 0
run_probe sdl-gl-direct sdl_gl_probe 0
run_probe sdl-renderer-direct sdl_renderer_probe 0
run_probe sdl-gl-steam sdl_gl_probe 1
run_probe sdl-renderer-steam sdl_renderer_probe 1

printf '%s\n' 'OpenGL Replay readback foundation passed.'
