#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
shader_directory="$workspace_root/crates/redunar-capture-vulkan/src/shaders"
mode=${1:---check}

case "$mode" in
  --check|--recompile|--update) ;;
  *)
    printf '%s\n' \
      'usage: tools/check-replay-shader.sh [--check|--recompile|--update]' >&2
    exit 2
    ;;
esac

validate_checked_in_binary() {
  local source_name=$1
  local binary_name=$2
  local manifest_name=$3
  sha256sum --check --strict "$manifest_name"

  binary_size=$(stat -c '%s' "$binary_name")
  if (( binary_size < 20 || binary_size % 4 != 0 )); then
    printf 'invalid SPIR-V byte length: %s\n' "$binary_size" >&2
    exit 1
  fi

  magic=$(od -An -tx4 -N4 "$binary_name" | tr -d '[:space:]')
  if [[ "$magic" != "07230203" ]]; then
    printf 'invalid SPIR-V magic: %s\n' "$magic" >&2
    exit 1
  fi

  if command -v spirv-val >/dev/null 2>&1; then
    spirv-val --target-env vulkan1.0 "$binary_name"
  fi
}

cd -- "$shader_directory"
shader_stems=(replay_rgba_to_nv12 replay_kms_rgba_to_nv12)
for stem in "${shader_stems[@]}"; do
  validate_checked_in_binary "$stem.comp" "$stem.comp.spv" "$stem.sha256"
done
if [[ "$mode" == "--check" ]]; then
  exit 0
fi

if ! command -v glslc >/dev/null 2>&1; then
  printf '%s\n' \
    'glslc is required to recompile shaders; install a Vulkan SDK development toolchain.' >&2
  exit 1
fi

temporary_directory=$(mktemp -d)
trap 'rm -rf -- "$temporary_directory"' EXIT
for stem in "${shader_stems[@]}"; do
  source_name="$stem.comp"
  binary_name="$stem.comp.spv"
  manifest_name="$stem.sha256"
  candidate="$temporary_directory/$binary_name"

  # Keep input names and Shaderc target stable: both affect compiler output.
  glslc \
    --target-env=vulkan1.0 \
    -fshader-stage=compute \
    "$source_name" \
    -o "$candidate"

  if command -v spirv-val >/dev/null 2>&1; then
    spirv-val --target-env vulkan1.0 "$candidate"
  fi

  if [[ "$mode" == "--recompile" ]]; then
    if ! cmp -s "$candidate" "$binary_name"; then
      printf 'recompiled SPIR-V differs for %s; review and update it explicitly.\n' "$source_name" >&2
      sha256sum "$source_name" "$binary_name" "$candidate" >&2
      exit 1
    fi
    continue
  fi

  install -m 0644 "$candidate" "$binary_name"
  manifest_candidate="$temporary_directory/$manifest_name"
  sha256sum "$source_name" "$binary_name" >"$manifest_candidate"
  install -m 0644 "$manifest_candidate" "$manifest_name"
  validate_checked_in_binary "$source_name" "$binary_name" "$manifest_name"
done

if [[ "$mode" == "--recompile" ]]; then
  printf '%s\n' 'Replay compute shaders recompile byte-for-byte.'
else
  printf '%s\n' 'Updated the checked-in Replay compute shaders and verification hashes.'
fi
