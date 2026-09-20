#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_directory=${1:-"$workspace_root/target/packages"}

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace_root/$output_directory"
fi
command -v rpmbuild >/dev/null 2>&1 || {
  printf '%s\n' 'rpmbuild is required to build the openSUSE RPM' >&2
  exit 1
}

mkdir -p "$workspace_root/target"
build_root=$(mktemp -d "$workspace_root/target/.tauri-opensuse-rpm-build.XXXXXX")
trap 'rm -rf -- "$build_root"' EXIT
mkdir -p "$build_root/tmp" "$build_root/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
export TMPDIR="$build_root/tmp"

package_root="$build_root/redunar-app-package-root"
"$workspace_root/tools/stage-tauri-package.sh" "$package_root"
tar -C "$build_root" -czf \
  "$build_root/rpmbuild/SOURCES/redunar-app-package-root.tar.gz" \
  redunar-app-package-root
install -m 0644 "$workspace_root/packaging/redunar-app.spec" \
  "$build_root/rpmbuild/SPECS/redunar-app.spec"

rpmbuild --define "_topdir $build_root/rpmbuild" \
  --define "_tmppath $build_root/tmp" \
  --define 'suse_version 1600' \
  --define 'dist .opensuse' \
  -bb "$build_root/rpmbuild/SPECS/redunar-app.spec"

rpm_path=$(find "$build_root/rpmbuild/RPMS" -type f -name 'redunar-app-*.rpm' -print -quit)
if [[ -z "$rpm_path" ]]; then
  printf '%s\n' 'rpmbuild did not produce an openSUSE Redunar RPM' >&2
  exit 1
fi
requirements=$(rpm -qp --requires "$rpm_path")
for requirement in gtk3 libwebkit2gtk-4_1-0 libdrm2 /usr/bin/parec /usr/bin/pactl libopus0 gstreamer-plugins-libav udev; do
  grep -Fxq "$requirement" <<<"$requirements" || {
    printf 'openSUSE RPM is missing dependency %s\n' "$requirement" >&2
    exit 1
  }
done
if grep -Fq 'libavcodec-freeworld' <<<"$requirements"; then
  printf '%s\n' 'openSUSE RPM must not contain Fedora codec capability dependencies' >&2
  exit 1
fi

mkdir -p "$output_directory"
final_path="$output_directory/redunar-app-linux-opensuse-x86_64.rpm"
install -m 0644 "$rpm_path" "$final_path"
printf 'Built native Redunar openSUSE RPM: %s\n' "$final_path"
