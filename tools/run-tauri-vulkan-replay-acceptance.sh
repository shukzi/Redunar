#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
tool_home=${HOME:?HOME must be set before running this acceptance test}
layer="$workspace_root/output/tauri-redunar/src-tauri/target/release/libredunar_capture_vulkan.so"

if [[ ! -f "$layer" ]]; then
  printf 'Build the Tauri release first; capture layer is missing at %s\n' "$layer" >&2
  exit 1
fi

run_id=$(date +%s%N)
test_home="/tmp/rdr-replay-home-$run_id"
test_state="/tmp/rdr-replay-state-$run_id"
test_config="/tmp/rdr-replay-config-$run_id"
mkdir -p "$test_home" "$test_state" "$test_config"
trap 'rm -rf -- "$test_home" "$test_state" "$test_config"' EXIT

cd -- "$workspace_root"
HOME="$test_home" \
XDG_STATE_HOME="$test_state" \
XDG_CONFIG_HOME="$test_config" \
RUSTUP_HOME="${RUSTUP_HOME:-$tool_home/.rustup}" \
CARGO_HOME="${CARGO_HOME:-$tool_home/.cargo}" \
REDUNAR_TAURI_CAPTURE_LAYER="$layer" \
  cargo test --offline --release \
    --manifest-path output/tauri-redunar/src-tauri/Cargo.toml \
    replay_enabled_vulkan_fixture_saves_clip_through_tauri_session_supervisor \
    -- --ignored --nocapture --test-threads=1
