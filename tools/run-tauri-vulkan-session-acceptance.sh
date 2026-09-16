#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
layer="$workspace_root/output/tauri-redunar/src-tauri/target/release/libredunar_capture_vulkan.so"

if [[ ! -f "$layer" ]]; then
  printf 'Build the Tauri release first; capture layer is missing at %s\n' "$layer" >&2
  exit 1
fi

cd -- "$workspace_root"
REDUNAR_TAURI_CAPTURE_LAYER="$layer" \
  cargo test --offline \
    --manifest-path output/tauri-redunar/src-tauri/Cargo.toml \
    capture_enabled_vulkan_fixture_runs_through_tauri_session_supervisor \
    -- --ignored --nocapture --test-threads=1
