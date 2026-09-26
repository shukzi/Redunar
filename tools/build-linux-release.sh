#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
local_build_root="$workspace_root/.redunar-build/linux"
source "$workspace_root/tools/lib/release-paths.sh"
if [[ -n ${REDUNAR_RELEASE_ROOT:-} ]]; then
  printf "%s\n" "Compatibility builds use the checkout-owned release path; unset REDUNAR_RELEASE_ROOT." >&2
  exit 2
fi
containerfile="$workspace_root/packaging/build-images/glibc-2.36.Containerfile"
image_name=localhost/redunar-build-glibc-2.36:bookworm
maximum_glibc=2.36
cargo_home=${CARGO_HOME:-"$HOME/.cargo"}

for command_name in podman objdump sha256sum flock python3; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'required compatibility-build command is unavailable: %s\n' "$command_name" >&2
    exit 1
  fi
done
node_modules=$(realpath -e "$workspace_root/output/tauri-redunar/node_modules")
if [[ ! -d "$node_modules" ]]; then
  printf '%s\n' 'frontend dependencies are missing; run npm install in output/tauri-redunar first' >&2
  exit 1
fi
if [[ ! -d "$cargo_home/registry" ]]; then
  printf '%s\n' 'the local Cargo registry is missing; fetch locked dependencies before building offline' >&2
  exit 1
fi

# One compatibility build per checkout; independent worktrees own different
# locks and targets. Never delete the host's native or validation Cargo cache.
mkdir -p "$local_build_root"
exec 9>"$local_build_root/build.lock"
flock -n 9 || { printf '%s\n' 'A compatibility build is already running in this checkout.' >&2; exit 1; }
rm -f -- "$local_build_root/runtime.sha256"
snapshot=$(mktemp -d "$workspace_root/.redunar-build/source.XXXXXX")
child_pid=
cleanup() {
  result=$?
  trap - EXIT INT TERM
  if [[ -n "$child_pid" ]]; then kill -TERM "$child_pid" 2>/dev/null || true; fi
  # Stop only the container started by this invocation before removing its
  # mounted source. Never stop another task's container or a user's app.
  if [[ -s "$snapshot/container.id" ]]; then
    podman stop --ignore --time 5 "$(cat "$snapshot/container.id")" >/dev/null 2>&1 || true
  fi
  if [[ -n "$child_pid" ]]; then
    kill -KILL "$child_pid" 2>/dev/null || true
    wait "$child_pid" 2>/dev/null || true
  fi
  rm -rf -- "$snapshot"
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
python3 "$workspace_root/tools/source-snapshot.py" "$workspace_root" "$snapshot" "$local_build_root/source-manifest.json"
cp -a "$node_modules" "$snapshot/output/tauri-redunar/node_modules"

podman build --pull=never --network=none --tag "$image_name" \
  --iidfile "$snapshot/image.id" --file "$containerfile" \
  "$(dirname -- "$containerfile")" &
child_pid=$!
wait "$child_pid"
child_pid=
image_id=$(cat "$snapshot/image.id")

container_arguments=(
  --rm
  --cidfile "$snapshot/container.id"
  --network=none
  --volume "$snapshot:/workspace:Z"
  --volume "$local_build_root:/build:Z"
  --volume "$cargo_home/registry:/usr/local/cargo/registry:ro,z"
  --env REDUNAR_NATIVE_BUILD_ROOT=/build/native
)
if [[ -d "$cargo_home/git" ]]; then
  container_arguments+=(--volume "$cargo_home/git:/usr/local/cargo/git:ro,z")
fi

podman run "${container_arguments[@]}" "$image_id" \
  python3 output/tauri-redunar/build-native.py &
child_pid=$!
wait "$child_pid"
child_pid=

artifacts=(
  "$release_root/redunar-tauri"
  "$release_root/redunar-steam-launch"
  "$release_root/redunar-hotkey-helper"
  "$release_root/redunar-update-helper"
  "$release_root/libredunar_capture_vulkan.so"
  "$release_root/libredunar_capture_opengl.so"
)

report="$local_build_root/linux-release-baseline.txt"
{
  printf 'source_commit=%s\n' "$(git -C "$workspace_root" rev-parse HEAD)"
  printf 'build_environment=Debian 12\n'
  printf 'maximum_glibc=%s\n' "$maximum_glibc"
  printf 'container_image=%s\n' "$image_name"
  printf 'container_image_id=%s\n' "$image_id"
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

sha256sum "${artifacts[@]}" >"$local_build_root/runtime.sha256"

printf 'Built Redunar release binaries against glibc %s; report: %s\n' \
  "$maximum_glibc" "$report"
