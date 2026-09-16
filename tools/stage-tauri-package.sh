#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
destination=${1:-}

if [[ -z "$destination" || "$destination" != /* || "$destination" == "/" ]]; then
  printf '%s\n' 'usage: tools/stage-tauri-package.sh /absolute/empty/staging/root' >&2
  exit 2
fi
if [[ -e "$destination" ]] && [[ -n "$(find "$destination" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  printf 'staging root must be empty: %s\n' "$destination" >&2
  exit 2
fi

cd -- "$workspace_root"
native_root="$workspace_root/output/tauri-redunar"
release_root="$native_root/src-tauri/target/release"

required_artifacts=(
  "$release_root/redunar-tauri"
  "$release_root/redunar-steam-launch"
  "$release_root/libredunar_capture_vulkan.so"
  "$release_root/redunar-hotkey-helper"
)
for artifact in "${required_artifacts[@]}"; do
  if [[ ! -f "$artifact" ]]; then
    printf 'missing Tauri release artifact: %s\n' "$artifact" >&2
    exit 1
  fi
done

install -d -m 0755 \
  "$destination/usr/bin" \
  "$destination/usr/libexec" \
  "$destination/usr/share/applications" \
  "$destination/usr/share/metainfo" \
  "$destination/usr/lib/udev/rules.d" \
  "$destination/usr/share/icons/hicolor/scalable/apps" \
  "$destination/usr/share/doc/redunar"

for icon_size in 16 32 48 64 128 256 512 1024; do
  install -d -m 0755 \
    "$destination/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps"
done

# The Tauri binary embeds the frontend. These sidecars stay beside it so the
# daemon can resolve the private Vulkan capture library and Steam bridge from
# the installed executable directory without a global layer or shell command.
install -m 0755 "$release_root/redunar-tauri" "$destination/usr/bin/redunar-tauri"
install -m 0755 "$release_root/redunar-steam-launch" \
  "$destination/usr/bin/redunar-steam-launch"
install -m 0755 "$release_root/libredunar_capture_vulkan.so" \
  "$destination/usr/bin/libredunar_capture_vulkan.so"
install -m 0755 "$release_root/redunar-hotkey-helper" \
  "$destination/usr/libexec/redunar-hotkey-helper"

install -m 0644 packaging/com.redunar.Redunar.desktop \
  "$destination/usr/share/applications/com.redunar.Redunar.desktop"
install -m 0644 packaging/com.redunar.Redunar.metainfo.xml \
  "$destination/usr/share/metainfo/com.redunar.Redunar.metainfo.xml"
install -m 0644 packaging/70-redunar-hotkeys.rules \
  "$destination/usr/lib/udev/rules.d/70-redunar-hotkeys.rules"
install -m 0644 assets/branding/redunar-logo.svg \
  "$destination/usr/share/icons/hicolor/scalable/apps/com.redunar.Redunar.svg"
for icon_size in 16 32 48 64 128 256 512 1024; do
  install -m 0644 "assets/branding/redunar-logo-${icon_size}.png" \
    "$destination/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps/com.redunar.Redunar.png"
done
install -m 0644 packaging/LICENSE-Lucide.txt \
  "$destination/usr/share/doc/redunar/LICENSE-Lucide.txt"
install -m 0644 crates/redunar-core/src/LICENSE-Noto-Sans-Mono.txt \
  "$destination/usr/share/doc/redunar/LICENSE-Noto-Sans-Mono.txt"
install -m 0644 packaging/THIRD-PARTY-NOTICES.md \
  "$destination/usr/share/doc/redunar/THIRD-PARTY-NOTICES.md"
install -m 0644 packaging/DEPENDENCY-LICENSES.md \
  "$destination/usr/share/doc/redunar/DEPENDENCY-LICENSES.md"

install -d -m 0755 "$destination/usr/share/licenses/redunar"
install -m 0644 LICENSE "$destination/usr/share/licenses/redunar/LICENSE"
install -m 0644 COPYRIGHT "$destination/usr/share/licenses/redunar/COPYRIGHT"

install -m 0644 "$native_root/licenses/libc/LICENSE-MIT" "$destination/usr/share/doc/redunar/LICENSE-libc-MIT.txt"
install -m 0644 "$native_root/licenses/libc/LICENSE-APACHE" "$destination/usr/share/doc/redunar/LICENSE-libc-APACHE.txt"
install -m 0644 "$native_root/licenses/tauri-api/LICENSE-MIT" "$destination/usr/share/doc/redunar/LICENSE-Tauri-API-MIT.txt"
install -m 0644 "$native_root/licenses/tauri-api/LICENSE-APACHE-2.0" "$destination/usr/share/doc/redunar/LICENSE-Tauri-API-APACHE-2.0.txt"

helper="$destination/usr/libexec/redunar-hotkey-helper"
if [[ "$(stat -c '%a' "$helper")" != "755" ]]; then
  printf '%s\n' 'staged Tauri shortcut helper must have mode 0755' >&2
  exit 1
fi
if [[ -u "$helper" || -g "$helper" ]]; then
  printf '%s\n' 'staged Tauri shortcut helper must not have setuid or setgid bits' >&2
  exit 1
fi
printf 'Staged Tauri Redunar package root at %s\n' "$destination"
