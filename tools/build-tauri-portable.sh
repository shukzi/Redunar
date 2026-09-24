#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_directory=${1:-"$workspace_root/target/packages"}

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace_root/$output_directory"
fi

mkdir -p "$workspace_root/target"
build_root=$(mktemp -d "$workspace_root/target/.tauri-portable-build.XXXXXX")
trap 'rm -rf -- "$build_root"' EXIT

package_root="$build_root/package"
"$workspace_root/tools/stage-tauri-package.sh" "$package_root"

mkdir -p "$output_directory"
final_path="$output_directory/redunar-app-linux-x86_64.tar.gz"
rm -f -- "$final_path"
tar --sort=name --mtime='UTC 2026-09-14' --owner=0 --group=0 --numeric-owner \
  -C "$package_root" -czf "$final_path" usr

archive_listing=$(tar -tzf "$final_path")
for required_path in \
  usr/bin/redunar-tauri \
  usr/bin/redunar-steam-launch \
  usr/bin/libredunar_capture_vulkan.so \
  usr/bin/libredunar_capture_opengl.so \
  usr/libexec/redunar-hotkey-helper \
  usr/lib/udev/rules.d/70-redunar-hotkeys.rules
do
  if ! grep -Fxq "$required_path" <<<"$archive_listing"; then
    printf 'portable package is missing %s\n' "$required_path" >&2
    exit 1
  fi
done

printf 'Built portable Tauri Redunar payload: %s\n' "$final_path"
