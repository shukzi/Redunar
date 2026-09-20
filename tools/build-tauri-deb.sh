#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_directory=${1:-"$workspace_root/target/packages"}

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace_root/$output_directory"
fi

for command_name in ar awk gzip md5sum tar; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'required DEB build command is unavailable: %s\n' "$command_name" >&2
    exit 1
  fi
done

version=$(awk '$1 == "Version:" { print $2; exit }' "$workspace_root/packaging/redunar-app.spec")
release=$(awk '$1 == "Release:" { print $2; exit }' "$workspace_root/packaging/redunar-app.spec")
release=${release%%.local*}
if [[ -z "$version" || -z "$release" || ! "$release" =~ ^[0-9][0-9.]*$ ]]; then
  printf '%s\n' 'could not derive the DEB version from packaging/redunar-app.spec' >&2
  exit 1
fi
package_version="$version-$release"

mkdir -p "$workspace_root/target"
build_root=$(mktemp -d "$workspace_root/target/.tauri-deb-build.XXXXXX")
trap 'rm -rf -- "$build_root"' EXIT

data_root="$build_root/data"
control_root="$build_root/control"
mkdir -p "$control_root"
"$workspace_root/tools/stage-tauri-package.sh" "$data_root"

installed_size=$(du -sk "$data_root" | awk '{ print $1 }')
cat >"$control_root/control" <<EOF
Package: redunar-app
Version: $package_version
Section: games
Priority: optional
Architecture: amd64
Maintainer: Redunar Project <maintainers@redunar.invalid>
Installed-Size: $installed_size
Depends: libgtk-3-0t64 | libgtk-3-0, libwebkit2gtk-4.1-0, libdrm2, pipewire-bin, libopus0, ffmpeg, gstreamer1.0-libav, udev
Conflicts: redunar, redunar-tauri
Replaces: redunar, redunar-tauri
Description: Local-first Linux gaming metrics and instant replay
 Redunar provides local game profiles, frame and system measurements,
 an in-game metrics overlay, and capability-gated instant replay.
EOF

cat >"$control_root/postinst" <<'EOF'
#!/bin/sh
set -e
if [ -x /usr/bin/udevadm ]; then
  /usr/bin/udevadm control --reload-rules >/dev/null 2>&1 || :
  /usr/bin/udevadm trigger --subsystem-match=input --sysname-match='event*' --property-match=ID_INPUT_KEYBOARD=1 --action=change >/dev/null 2>&1 || :
  /usr/bin/udevadm trigger --subsystem-match=input --sysname-match='event*' --property-match=ID_INPUT_MOUSE=1 --action=change >/dev/null 2>&1 || :
fi
exit 0
EOF
chmod 0755 "$control_root/postinst"

(
  cd "$data_root"
  find usr -type f -print0 | sort -z | xargs -0 md5sum
) >"$control_root/md5sums"

printf '2.0\n' >"$build_root/debian-binary"
tar --sort=name --mtime='UTC 2026-09-14' --owner=0 --group=0 --numeric-owner \
  -C "$control_root" -czf "$build_root/control.tar.gz" .
tar --sort=name --mtime='UTC 2026-09-14' --owner=0 --group=0 --numeric-owner \
  -C "$data_root" -czf "$build_root/data.tar.gz" .

mkdir -p "$output_directory"
final_path="$output_directory/redunar-app-linux-amd64.deb"
rm -f -- "$final_path"
(
  cd "$build_root"
  ar rcsD "$final_path" debian-binary control.tar.gz data.tar.gz
)

mapfile -t members < <(ar t "$final_path")
expected_members=(debian-binary control.tar.gz data.tar.gz)
if [[ "${members[*]}" != "${expected_members[*]}" ]]; then
  printf 'unexpected DEB members: %s\n' "${members[*]}" >&2
  exit 1
fi
control_metadata=$(tar -xOzf "$build_root/control.tar.gz" ./control)
if ! grep -Fxq 'Package: redunar-app' <<<"$control_metadata"; then
  printf '%s\n' 'generated DEB control metadata is invalid' >&2
  exit 1
fi

printf 'Built local Tauri Redunar DEB: %s\n' "$final_path"
