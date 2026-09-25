#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_directory=${1:-"$workspace_root/target/install-assets"}
private_key=${2:-${REDUNAR_RELEASE_SIGNING_KEY:-}}
public_key=${3:-"$workspace_root/packaging/release-signing-public.pem"}

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace_root/$output_directory"
fi

mkdir -p "$output_directory"
find "$output_directory" -maxdepth 1 -type f \( \
  -name 'redunar-app-linux-*' -o -name '*.sha256' -o \
  -name 'SHA256SUMS' -o -name 'SHA256SUMS.sig' -o -name 'VERSION' \
\) -delete

if [[ -z "$private_key" || "$private_key" != /* ]]; then
  printf '%s\n' 'build-install-assets.sh requires an absolute release private-key path as its second argument or REDUNAR_RELEASE_SIGNING_KEY' >&2
  exit 2
fi

"$workspace_root/tools/build-linux-release.sh"

installer_glibc=$(awk -F= '$1 == "minimum_glibc" { print $2; exit }' "$workspace_root/install.sh")
if [[ ! "$installer_glibc" =~ ^[0-9]+\.[0-9]+$ ]]; then
  printf '%s\n' 'could not read the installer glibc baseline' >&2
  exit 1
fi
for binary in \
  "$workspace_root/output/tauri-redunar/src-tauri/target/release/redunar-tauri" \
  "$workspace_root/output/tauri-redunar/src-tauri/target/release/redunar-steam-launch" \
  "$workspace_root/output/tauri-redunar/src-tauri/target/release/redunar-hotkey-helper" \
  "$workspace_root/output/tauri-redunar/src-tauri/target/release/redunar-update-helper" \
  "$workspace_root/output/tauri-redunar/src-tauri/target/release/libredunar_capture_vulkan.so" \
  "$workspace_root/output/tauri-redunar/src-tauri/target/release/libredunar_capture_opengl.so"
do
  if [[ ! -f "$binary" ]]; then
    printf 'release component is missing: %s\n' "$binary" >&2
    exit 1
  fi
  required_glibc=$(objdump -T "$binary" \
    | sed -n 's/.*(GLIBC_\([0-9.]*\)).*/\1/p' \
    | sort -V \
    | tail -n 1)
  if [[ -n "$required_glibc" && "$(printf '%s\n%s\n' "$required_glibc" "$installer_glibc" | sort -V | tail -n 1)" != "$installer_glibc" ]]; then
    printf '%s requires glibc %s but install.sh permits %s\n' \
      "$(basename -- "$binary")" "$required_glibc" "$installer_glibc" >&2
    exit 1
  fi
done

rpm_directory=$(mktemp -d "$workspace_root/target/.install-rpm.XXXXXX")
trap 'rm -rf -- "$rpm_directory"' EXIT
"$workspace_root/tools/build-local-tauri-rpm.sh" "$rpm_directory"
install -m 0644 "$rpm_directory/redunar-app.rpm" \
  "$output_directory/redunar-app-linux-x86_64.rpm"
"$workspace_root/tools/build-tauri-deb.sh" "$output_directory"
"$workspace_root/tools/build-tauri-arch-package.sh" "$output_directory"
"$workspace_root/tools/build-tauri-opensuse-rpm.sh" "$output_directory"
"$workspace_root/tools/build-tauri-portable.sh" "$output_directory"

for artifact in \
  "$output_directory/redunar-app-linux-x86_64.rpm" \
  "$output_directory/redunar-app-linux-amd64.deb" \
  "$output_directory/redunar-app-linux-x86_64.pkg.tar.zst" \
  "$output_directory/redunar-app-linux-opensuse-x86_64.rpm" \
  "$output_directory/redunar-app-linux-x86_64.tar.gz"
do
  (
    cd "$output_directory"
    sha256sum "$(basename -- "$artifact")" >"$(basename -- "$artifact").sha256"
  )
done

"$workspace_root/tools/write-release-version.sh" "$output_directory"
"$workspace_root/tools/sign-release-assets.sh" "$output_directory" "$private_key" "$public_key"

printf 'Built Redunar installer assets in %s\n' "$output_directory"
