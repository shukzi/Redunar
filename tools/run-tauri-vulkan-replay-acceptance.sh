#!/usr/bin/env bash
set -euo pipefail


workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
tool_home=${HOME:?HOME must be set before running this acceptance test}
beta_access=0
if [[ $# -gt 1 || ( $# -eq 1 && "$1" != --nvidia-beta ) ]]; then
  printf '%s\n' 'usage: tools/run-tauri-vulkan-replay-acceptance.sh [--nvidia-beta]' >&2
  exit 2
fi
if [[ $# -eq 1 ]]; then beta_access=1; fi
source "$workspace_root/tools/lib/release-paths.sh"
verify_release_artifacts
layer="$release_root/libredunar_capture_vulkan.so"

if [[ ! -f "$layer" ]]; then
  printf 'Build the Tauri release first; capture layer is missing at %s\n' "$layer" >&2
  exit 1
fi

# Compile our finite Vulkan 1.1 scene rather than relying on distro vkcube's
# unstated API version. This is a manual hardware runner, not a package input.
fixture_dir="$workspace_root/.redunar-build/native/fixtures"
mkdir -p "$fixture_dir"
read -r -a fixture_flags <<< "$(pkg-config --cflags --libs vulkan xcb)"
cc -std=c11 -Wall -Wextra -Werror "$workspace_root/tools/fixtures/vulkan_scene.c" \
  -o "$fixture_dir/vulkan-scene" "${fixture_flags[@]}"

test_root=$(mktemp -d /tmp/rdr-replay.XXXXXX)
test_home="$test_root/home"
test_state="$test_root/state"
test_config="$test_root/config"
test_runtime="$test_root/runtime"
mkdir -m 700 "$test_home" "$test_state" "$test_config" "$test_runtime"
cleanup() {
  result=$?
  # Preserve the opt-in report even when startup or decode fails. Do not
  # retain synthetic media or expose it to ordinary user state.
  if [[ -f "$test_state/redunar/diagnostics.log" ]]; then
    mkdir -p "$workspace_root/.redunar-build/reports"
    report_dir=$(mktemp -d "$workspace_root/.redunar-build/reports/replay-acceptance.XXXXXX")
    cp -- "$test_state/redunar/diagnostics.log" "$report_dir/diagnostics.log" || true
    if [[ -f "$test_state/redunar/diagnostics.log.1" ]]; then
      cp -- "$test_state/redunar/diagnostics.log.1" "$report_dir/diagnostics.log.1" || true
    fi
    printf 'Diagnostic report: %s\n' "$report_dir"
  fi
  rm -rf -- "$test_root"
  exit "$result"
}
trap cleanup EXIT

cd -- "$workspace_root"
# Compilation is unbounded; only the owned live fixture gets a hard deadline.
cargo test --locked --offline --release --target-dir "$workspace_root/.redunar-build/native/tauri" \
  --manifest-path output/tauri-redunar/src-tauri/Cargo.toml \
  replay_enabled_vulkan_fixture_saves_clip_through_tauri_session_supervisor --no-run
HOME="$test_home" \
XDG_STATE_HOME="$test_state" \
XDG_CONFIG_HOME="$test_config" \
XDG_RUNTIME_DIR="$test_runtime" \
XDG_CACHE_HOME="$test_root/cache" \
REDUNAR_TAURI_TEST_STATE="$test_state/redunar" \
REDUNAR_TAURI_BETA_ACCESS="$beta_access" \
RUSTUP_HOME="${RUSTUP_HOME:-$tool_home/.rustup}" \
CARGO_HOME="${CARGO_HOME:-$tool_home/.cargo}" \
REDUNAR_TAURI_CAPTURE_LAYER="$layer" \
REDUNAR_TAURI_VULKAN_SCENE="$fixture_dir/vulkan-scene" \
  timeout --kill-after=5s 75s cargo test --locked --offline --release --target-dir "$workspace_root/.redunar-build/native/tauri" \
    --manifest-path output/tauri-redunar/src-tauri/Cargo.toml \
    replay_enabled_vulkan_fixture_saves_clip_through_tauri_session_supervisor \
    -- --ignored --nocapture --test-threads=1
