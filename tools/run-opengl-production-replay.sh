#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
probe="$workspace_root/target/release/examples/capture_probe"
examples="$workspace_root/target/release/examples"
logs=$(mktemp -d "${TMPDIR:-/tmp}/redunar-production-replay.XXXXXX")
review_dir=${REDUNAR_OPENGL_REVIEW_DIR:-}
if [[ -n $review_dir ]]; then
  if [[ $review_dir != /* ]]; then
    printf '%s\n' 'REDUNAR_OPENGL_REVIEW_DIR must be an absolute directory path.' >&2
    exit 2
  fi
  mkdir -p -- "$review_dir"
fi
states=()
cleanup() {
  for state in "${states[@]}"; do
    rm -rf -- "$state"
  done
  rm -rf -- "$logs"
}
trap cleanup EXIT

cd -- "$workspace_root"
cargo build --release --offline \
  -p redunar-capture-opengl --lib --examples \
  -p redunar-daemon --example capture_probe

run_case() {
  local name=$1 target=$2 fps=$3 format=$4 overlay=$5
  local log="$logs/$name.log"
  REDUNAR_CAPTURE_PROBE_PRODUCTION_REPLAY=1 \
  REDUNAR_CAPTURE_PROBE_KEEP_STATE=1 \
  REDUNAR_CAPTURE_PROBE_OVERLAY="$overlay" \
  REDUNAR_CAPTURE_PROBE_FRAMES=600 \
  REDUNAR_CAPTURE_PROBE_WIDTH=1920 \
  REDUNAR_CAPTURE_PROBE_HEIGHT=1080 \
  REDUNAR_CAPTURE_PROBE_REPLAY_FPS="$fps" \
  REDUNAR_CAPTURE_PROBE_REPLAY_FORMAT="$format" \
  REDUNAR_CAPTURE_PROBE_SAVE_VIA_MENU="$overlay" \
  REDUNAR_CAPTURE_PROBE_ORIENTATION_PATTERN=1 \
  REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS=35 \
    "$probe" "$examples/$target" >"$log" 2>&1 || {
      cat "$log" >&2
      local failed_state
      failed_state=$(sed -n 's/^Redunar capture probe retained state at //p' "$log")
      [[ -z $failed_state ]] || states+=("$failed_state")
      return 1
    }

  local state clip
  state=$(sed -n 's/^Redunar capture probe retained state at //p' "$log")
  [[ -n $state ]]
  states+=("$state")

  grep -Fxq 'phase=Completed' "$log"
  grep -Fxq 'capture_api=Some(OpenGl)' "$log"
  grep -Fxq 'drops=0' "$log"
  grep -Fxq 'rejects=0' "$log"
  grep -Fxq 'production_replay_buffering=true' "$log"
  grep -Fxq 'production_replay_saving=true' "$log"
  grep -Fxq 'production_replay_menu_control=true' "$log"
  grep -Fxq 'production_replay_visibility_toggle=true' "$log"
  grep -Fxq 'production_replay_saved_notice=true' "$log"
  grep -Fxq 'production_replay_completed_save_revision=1' "$log"
  grep -Fxq "replay_candidate_target_fps=$fps" "$log"
  local fds_before fds_after threads_before threads_after
  fds_before=$(sed -n 's/^production_replay_fds_before=//p' "$log")
  fds_after=$(sed -n 's/^production_replay_fds_after=//p' "$log")
  threads_before=$(sed -n 's/^production_replay_threads_before=//p' "$log")
  threads_after=$(sed -n 's/^production_replay_threads_after=//p' "$log")
  [[ $fds_before =~ ^[0-9]+$ && $fds_after =~ ^[0-9]+$ ]]
  [[ $threads_before =~ ^[0-9]+$ && $threads_after =~ ^[0-9]+$ ]]
  ((fds_after <= fds_before + 4))
  ((threads_after <= threads_before + 1))
  if [[ $overlay == 1 ]]; then
    grep -Fxq 'overlay_status=Some(Active)' "$log"
  fi
  clip=$(sed -n 's/^replay_encoded_clip=//p' "$log")
  [[ -s $clip && $clip == *.$format ]]
  ffprobe -v error -show_entries stream=codec_name,width,height \
    -of default=noprint_wrappers=1 "$clip" >"$logs/$name.streams"
  grep -Fxq 'codec_name=h264' "$logs/$name.streams"
  grep -Fxq 'codec_name=opus' "$logs/$name.streams"
  grep -Fxq 'width=1920' "$logs/$name.streams"
  grep -Fxq 'height=1080' "$logs/$name.streams"
  ffmpeg -v error -i "$clip" -f null - >"$logs/$name.decode" 2>&1
  python3 - "$clip" "$fps" <<'PY'
import fractions, json, statistics, subprocess, sys
clip, expected = sys.argv[1], int(sys.argv[2])
report = json.loads(subprocess.check_output([
    'ffprobe', '-v', 'error', '-count_frames', '-show_streams',
    '-show_format', '-of', 'json', clip,
]))
video = next(s for s in report['streams'] if s['codec_type'] == 'video')
rate = float(fractions.Fraction(video['avg_frame_rate']))
duration = float(report['format']['duration'])
frames = int(video['nb_read_frames'])
assert abs(rate - expected) < expected * 0.05, (rate, expected)
assert 1.5 < duration < 15.5, duration
assert frames >= expected * 1.5, frames
packet_report = json.loads(subprocess.check_output([
    'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_packets',
    '-show_entries', 'packet=pts_time', '-of', 'json', clip,
]))
timestamps = [float(packet['pts_time']) for packet in packet_report['packets']]
deltas = [later - earlier for earlier, later in zip(timestamps, timestamps[1:])]
assert deltas and abs(statistics.median(deltas) - 1 / expected) < 0.001
# An overloaded presentation can skip capture slots; retained packets must
# still sit on the requested fixed-rate timestamp grid.
assert all(0 < delta <= 0.25 and abs(delta - round(delta * expected) / expected) < 0.001
           for delta in deltas), (min(deltas), max(deltas), expected)
keys = subprocess.check_output([
    'ffprobe', '-v', 'error', '-skip_frame', 'nokey', '-select_streams',
    'v:0', '-show_entries', 'frame=key_frame', '-of', 'csv=p=0', clip,
]).splitlines()
assert b'1' in keys, keys[:5]
PY
  ffmpeg -v error -i "$clip" -frames:v 1 -f rawvideo -pix_fmt rgb24 \
    "$logs/$name.rgb"
  python3 - "$logs/$name.rgb" <<'PY'
import pathlib, sys
rgb = pathlib.Path(sys.argv[1]).read_bytes()
width, height = 1920, 1080
assert len(rgb) == width * height * 3
def pixel(y):
    offset = (y * width + width // 2) * 3
    return rgb[offset:offset + 3]
top, bottom = pixel(height // 8), pixel(height * 7 // 8)
assert top[0] > top[2] + 40, (top, bottom)
assert bottom[2] > bottom[0] + 40, (top, bottom)
PY
  if [[ -n $review_dir ]]; then
    install -m 0644 -- "$clip" "$review_dir/$name.$format"
    ffmpeg -v error -i "$clip" -frames:v 1 \
      "$review_dir/$name.png"
  fi
  printf '%-28s ' "$name"
  awk -F= '/^(replay_exported_frames|production_replay_encoded_packets|replay_encoded_bytes)=/{printf "%s=%s ", $1, $2} END{print ""}' "$log"
}

run_case glx-production-1080p30-mkv glx_probe 30 mkv 1
run_case sdl-production-1080p60-mp4 sdl_gl_probe 60 mp4 0

fullscreen_log="$logs/glx-fullscreen.log"
REDUNAR_CAPTURE_PROBE_PRODUCTION_REPLAY=1 \
REDUNAR_CAPTURE_PROBE_KEEP_STATE=1 \
REDUNAR_CAPTURE_PROBE_OVERLAY=1 \
REDUNAR_CAPTURE_PROBE_FULLSCREEN=1 \
REDUNAR_CAPTURE_PROBE_SAVE_VIA_CONTROL=1 \
REDUNAR_CAPTURE_PROBE_FRAMES=600 \
REDUNAR_CAPTURE_PROBE_WIDTH=640 \
REDUNAR_CAPTURE_PROBE_HEIGHT=360 \
REDUNAR_CAPTURE_PROBE_TIMEOUT_SECONDS=35 \
  "$probe" "$examples/glx_probe" >"$fullscreen_log" 2>&1 || {
    cat "$fullscreen_log" >&2
    exit 1
  }
fullscreen_state=$(sed -n 's/^Redunar capture probe retained state at //p' "$fullscreen_log")
[[ -n $fullscreen_state ]]
states+=("$fullscreen_state")
grep -Fxq 'GLX probe fullscreen entered=true restored=true' "$fullscreen_log"
(( $(grep -Ec 'Redunar Replay: rolling recorder reset for [0-9]+x[0-9]+' "$fullscreen_log") >= 2 ))
grep -Fxq 'Redunar Replay: rolling recorder reset for 640x360' "$fullscreen_log"
grep -Fxq 'production_replay_completed_save_revision=1' "$fullscreen_log"
grep -Fxq 'drops=0' "$fullscreen_log"
grep -Fxq 'rejects=0' "$fullscreen_log"
fullscreen_clip=$(sed -n 's/^replay_encoded_clip=//p' "$fullscreen_log")
[[ -s $fullscreen_clip ]]
ffmpeg -v error -i "$fullscreen_clip" -f null - >"$logs/glx-fullscreen.decode" 2>&1
if [[ -n $review_dir ]]; then
  fullscreen_extension=${fullscreen_clip##*.}
  [[ $fullscreen_extension == mkv || $fullscreen_extension == mp4 ]]
  install -m 0644 -- "$fullscreen_clip" "$review_dir/glx-fullscreen.$fullscreen_extension"
fi
printf '%s\n' 'glx-fullscreen-restore        native save and codec resets passed'
printf '%s\n' 'OpenGL production Replay runtime and save acceptance passed.'
if [[ -n $review_dir ]]; then
  printf 'Review clips and frames: %s\n' "$review_dir"
fi
