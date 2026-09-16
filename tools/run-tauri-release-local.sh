#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
release_root="$workspace_root/output/tauri-redunar/src-tauri/target/release"
binary="$release_root/redunar-tauri"
helper="$release_root/redunar-hotkey-helper"

if [[ ! -x "$binary" || ! -x "$helper" || ! -f "$release_root/libredunar_capture_vulkan.so" || ! -x "$release_root/redunar-steam-launch" ]]; then
  printf '%s\n' 'Build the Tauri release first; one or more local runtime components are missing.' >&2
  exit 1
fi

# The release directory already contains the capture layer and Steam bridge.
# Point only the same-user shortcut monitor at its adjacent helper so this
# checkout can be tested without copying files into /usr or requesting auth.
exec env REDUNAR_HOTKEY_HELPER="$helper" "$binary" "$@"
