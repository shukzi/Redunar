#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
probe="$workspace_root/target/release/examples/capture_probe"
examples="$workspace_root/target/release/examples"
acceptance_root=$(mktemp -d "${TMPDIR:-/tmp}/redunar-opengl-acceptance.XXXXXX")
trap 'rm -rf -- "$acceptance_root"' EXIT

cd -- "$workspace_root"
cargo build --release --offline \
  -p redunar-capture-opengl --lib --examples \
  -p redunar-daemon --example capture_probe \
  -p redunar-platform --bin redunar-steam-launch

run_probe() {
  local name=$1
  local target=$2
  local layout=$3
  local preset=$4
  local steam_bridge=$5
  local metrics=${6:-255}
  local log="$acceptance_root/$name.log"

  REDUNAR_CAPTURE_PROBE_OVERLAY=1 \
  REDUNAR_CAPTURE_PROBE_LAYOUT="$layout" \
  REDUNAR_CAPTURE_PROBE_PRESET="$preset" \
  REDUNAR_CAPTURE_PROBE_METRICS="$metrics" \
  REDUNAR_CAPTURE_PROBE_SCALE=135 \
  REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS=15 \
  REDUNAR_CAPTURE_PROBE_STEAM_BRIDGE="$steam_bridge" \
    "$probe" "$examples/$target" >"$log" 2>&1

  grep -Fxq 'phase=Completed' "$log"
  grep -Fxq 'capture_api=Some(OpenGl)' "$log"
  grep -Eq '^frames=[1-9][0-9]*$' "$log"
  grep -Fxq 'drops=0' "$log"
  grep -Fxq 'rejects=0' "$log"
  grep -Fxq 'overlay_status=Some(Active)' "$log"

  printf '%-24s ' "$name"
  awk -F= '/^(frames|drops|rejects|overlay_status)=/{printf "%s=%s ", $1, $2} END{print ""}' "$log"
}

run_probe glx-ribbon-direct glx_probe ribbon compact 0
run_probe egl-telemetry-direct egl_probe telemetry custom 0 255
run_probe sdl-gl-grid-direct sdl_gl_probe grid detailed 0
run_probe sdl-renderer-fps-direct sdl_renderer_probe ribbon fps-only 0
run_probe sdl-gl-telemetry-steam sdl_gl_probe telemetry compact 1
run_probe sdl-renderer-grid-steam sdl_renderer_probe grid custom 1 255

printf '%s\n' 'OpenGL overlay acceptance passed.'
