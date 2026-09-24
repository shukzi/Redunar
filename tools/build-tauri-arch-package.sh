#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_directory=${1:-"$workspace_root/target/packages"}
arch_image='docker.io/library/archlinux@sha256:5b987b0196907ea97dd10e7b48e7d403c35c0389986da015aa86cf8d0b058084'

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace_root/$output_directory"
fi
command -v podman >/dev/null 2>&1 || {
  printf '%s\n' 'podman is required to build the native Arch package' >&2
  exit 1
}
if ! podman image exists "$arch_image"; then
  podman pull "$arch_image"
fi

version=$(awk '$1 == "Version:" { print $2; exit }' "$workspace_root/packaging/redunar-app.spec")
release=$(awk '$1 == "Release:" { print $2; exit }' "$workspace_root/packaging/redunar-app.spec")
release=${release%%.local*}
if [[ -z "$version" || -z "$release" || ! "$release" =~ ^[0-9][0-9.]*$ ]]; then
  printf '%s\n' 'could not derive the Arch package version from packaging/redunar-app.spec' >&2
  exit 1
fi

mkdir -p "$workspace_root/target"
build_root=$(mktemp -d "$workspace_root/target/.tauri-arch-build.XXXXXX")
trap 'rm -rf -- "$build_root"' EXIT
package_root="$build_root/redunar-app-package-root"
package_build_root="$build_root/package"
mkdir -p "$package_build_root/home"
"$workspace_root/tools/stage-tauri-package.sh" "$package_root"
tar --sort=name --mtime='UTC 2026-09-14' --owner=0 --group=0 --numeric-owner \
  -C "$package_root" -czf "$package_build_root/redunar-app-package-root.tar.gz" usr
package_root_sha256=$(sha256sum "$package_build_root/redunar-app-package-root.tar.gz" | awk '{print $1}')
sed \
  -e "s/@REDUNAR_ARCH_PKGVER@/$version/" \
  -e "s/@REDUNAR_ARCH_PKGREL@/$release/" \
  -e "s/@REDUNAR_PACKAGE_ROOT_SHA256@/$package_root_sha256/" \
  "$workspace_root/packaging/arch/PKGBUILD.in" >"$package_build_root/PKGBUILD"
install -m 0644 "$workspace_root/packaging/arch/redunar-app.install" \
  "$package_build_root/redunar-app.install"

podman run --rm --network=none --userns=keep-id \
  --user "$(id -u):$(id -g)" \
  --env HOME=/build/home \
  --volume "$package_build_root:/build:Z" \
  "$arch_image" \
  sh -c 'cd /build && makepkg --nodeps --noconfirm --cleanbuild --clean'

package_path=$(find "$package_build_root" -maxdepth 1 -type f -name 'redunar-app-*.pkg.tar.zst' -print -quit)
if [[ -z "$package_path" ]]; then
  printf '%s\n' 'makepkg did not produce a Redunar Arch package' >&2
  exit 1
fi
package_listing=$(tar --zstd -tf "$package_path")
for required_path in \
  .PKGINFO \
  usr/bin/redunar-tauri \
  usr/bin/redunar-steam-launch \
  usr/bin/libredunar_capture_vulkan.so \
  usr/bin/libredunar_capture_opengl.so \
  usr/libexec/redunar-hotkey-helper \
  usr/lib/udev/rules.d/70-redunar-hotkeys.rules
do
  grep -Fxq "$required_path" <<<"$package_listing" || {
    printf 'native Arch package is missing %s\n' "$required_path" >&2
    exit 1
  }
done

mkdir -p "$output_directory"
final_path="$output_directory/redunar-app-linux-x86_64.pkg.tar.zst"
install -m 0644 "$package_path" "$final_path"
printf 'Built native Redunar Arch package: %s\n' "$final_path"
