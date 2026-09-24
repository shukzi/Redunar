#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
project_root=$(dirname -- "$workspace_root")
local_build_root="$project_root/local-builds"
containerfile="$workspace_root/packaging/build-images/glibc-2.36.Containerfile"
image_name=localhost/redunar-build-glibc-2.36:bookworm
maximum_glibc=2.36

for command_name in podman objdump sha256sum; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'required compatibility-build command is unavailable: %s\n' "$command_name" >&2
    exit 1
  fi
done
if [[ ! -d "$local_build_root/node_modules" ]]; then
  printf '%s\n' 'frontend dependencies are missing; run npm install in output/tauri-redunar first' >&2
  exit 1
fi
if [[ ! -d "$HOME/.cargo/registry" ]]; then
  printf '%s\n' 'the local Cargo registry is missing; fetch locked dependencies before building offline' >&2
  exit 1
fi

podman build --pull=never --network=none --tag "$image_name" --file "$containerfile" \
  "$(dirname -- "$containerfile")"

mkdir -p "$local_build_root"
container_arguments=(
  --rm
  --network=none
  --volume "$workspace_root:/workspace:Z"
  --volume "$local_build_root:/local-builds:Z"
  --volume "$HOME/.cargo/registry:/usr/local/cargo/registry:ro,Z"
)
if [[ -d "$HOME/.cargo/git" ]]; then
  container_arguments+=(--volume "$HOME/.cargo/git:/usr/local/cargo/git:ro,Z")
fi

podman run "${container_arguments[@]}" "$image_name" sh -c '
  set -eu
  rm -rf /local-builds/capture-runtime /local-builds/tauri-target
  mkdir -p /local-builds/capture-runtime /local-builds/tauri-target
  python3 output/tauri-redunar/build-native.py
'

release_root="$workspace_root/output/tauri-redunar/src-tauri/target/release"
artifacts=(
  "$release_root/redunar-tauri"
  "$release_root/redunar-steam-launch"
  "$release_root/redunar-hotkey-helper"
  "$release_root/libredunar_capture_vulkan.so"
  "$release_root/libredunar_capture_opengl.so"
)

mkdir -p "$workspace_root/target"
report="$workspace_root/target/linux-release-baseline.txt"
{
  printf 'build_environment=Debian 12\n'
  printf 'maximum_glibc=%s\n' "$maximum_glibc"
  printf 'container_image=%s\n' "$image_name"
} >"$report"

for artifact in "${artifacts[@]}"; do
  if [[ ! -f "$artifact" ]]; then
    printf 'compatibility build did not produce %s\n' "$artifact" >&2
    exit 1
  fi
  required_glibc=$(objdump -T "$artifact" \
    | sed -n 's/.*(GLIBC_\([0-9.]*\)).*/\1/p' \
    | sort -V \
    | tail -n 1)
  if [[ -n "$required_glibc" && \
        "$(printf '%s\n%s\n' "$required_glibc" "$maximum_glibc" | sort -V | tail -n 1)" != "$maximum_glibc" ]]; then
    printf '%s requires glibc %s; the release baseline is %s\n' \
      "$(basename -- "$artifact")" "$required_glibc" "$maximum_glibc" >&2
    exit 1
  fi
  printf '%s glibc=%s sha256=%s\n' \
    "$(basename -- "$artifact")" "${required_glibc:-none}" \
    "$(sha256sum "$artifact" | awk '{print $1}')" >>"$report"
done

printf 'Built Redunar release binaries against glibc %s; report: %s\n' \
  "$maximum_glibc" "$report"
