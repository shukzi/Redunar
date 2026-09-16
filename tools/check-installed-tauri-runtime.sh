#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
package_path=$(find "$workspace_root/target/packages" -maxdepth 1 -type f \( -name 'redunar-app.rpm' -o -name 'redunar-app-*.rpm' \) -print 2>/dev/null | sort -V | tail -n 1)
if [[ -z "$package_path" ]]; then
  printf 'Verified Tauri RPM is missing under %s\n' "$workspace_root/target/packages" >&2
  exit 1
fi
payload_root=$(mktemp -d)
trap 'rm -rf -- "$payload_root"' EXIT
if ! rpm2cpio "$package_path" | cpio -idm --quiet -D "$payload_root"; then
  printf 'Could not extract the verified Tauri RPM payload: %s\n' "$package_path" >&2
  exit 1
fi

check_component() {
  local name=$1
  local built=$2
  local installed=$3
  if [[ ! -f "$built" ]]; then
    printf 'Build component missing: %s (%s)\n' "$name" "$built" >&2
    return 1
  fi
  if [[ ! -f "$installed" ]]; then
    printf 'Installed component missing: %s (%s)\n' "$name" "$installed" >&2
    return 1
  fi
  local built_hash installed_hash
  built_hash=$(sha256sum "$built" | cut -d' ' -f1)
  installed_hash=$(sha256sum "$installed" | cut -d' ' -f1)
  if [[ "$built_hash" != "$installed_hash" ]]; then
    printf 'Installed component is outdated: %s\n' "$name" >&2
    return 1
  fi
  printf 'Current: %s\n' "$name"
}

status=0
check_component redunar-tauri \
  "$payload_root/usr/bin/redunar-tauri" /usr/bin/redunar-tauri || status=1
check_component capture-layer \
  "$payload_root/usr/bin/libredunar_capture_vulkan.so" /usr/bin/libredunar_capture_vulkan.so || status=1
check_component steam-wrapper \
  "$payload_root/usr/bin/redunar-steam-launch" /usr/bin/redunar-steam-launch || status=1
check_component shortcut-helper \
  "$payload_root/usr/libexec/redunar-hotkey-helper" /usr/libexec/redunar-hotkey-helper || status=1
check_component desktop-entry \
  "$payload_root/usr/share/applications/com.redunar.Redunar.desktop" \
  /usr/share/applications/com.redunar.Redunar.desktop || status=1
check_component appstream-metadata \
  "$payload_root/usr/share/metainfo/com.redunar.Redunar.metainfo.xml" \
  /usr/share/metainfo/com.redunar.Redunar.metainfo.xml || status=1
check_component application-icon \
  "$payload_root/usr/share/icons/hicolor/scalable/apps/com.redunar.Redunar.svg" \
  /usr/share/icons/hicolor/scalable/apps/com.redunar.Redunar.svg || status=1

if [[ ! -f /usr/lib/udev/rules.d/70-redunar-hotkeys.rules ]]; then
  printf 'Installed component missing: keyboard uaccess rule (/usr/lib/udev/rules.d/70-redunar-hotkeys.rules)\n' >&2
  status=1
elif [[ "$(sha256sum "$payload_root/usr/lib/udev/rules.d/70-redunar-hotkeys.rules" | cut -d' ' -f1)" != "$(sha256sum /usr/lib/udev/rules.d/70-redunar-hotkeys.rules | cut -d' ' -f1)" ]]; then
  printf 'Installed component is outdated: keyboard uaccess rule\n' >&2
  status=1
else
  printf 'Current: keyboard uaccess rule\n'
fi

if ((status != 0)); then
  printf '%s\n' 'Installed Tauri runtime does not match the verified release build.' >&2
  exit "$status"
fi

printf '%s\n' 'Installed Tauri runtime matches the verified release build.'
