#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$workspace_root"
mkdir -p "$workspace_root/target"
release_check_root=$(mktemp -d "${TMPDIR:-/tmp}/redunar-release.XXXXXX")
trap 'rm -rf -- "$release_check_root"' EXIT
export TMPDIR="$release_check_root"

test -x tools/run-tauri-vulkan-session-acceptance.sh
bash -n tools/run-tauri-vulkan-session-acceptance.sh
test -x tools/run-tauri-vulkan-replay-acceptance.sh
bash -n tools/run-tauri-vulkan-replay-acceptance.sh
test -x tools/run-tauri-release-local.sh
bash -n tools/run-tauri-release-local.sh
test -x tools/run-opengl-overlay-acceptance.sh
bash -n tools/run-opengl-overlay-acceptance.sh
test -x tools/run-opengl-replay-foundation.sh
bash -n tools/run-opengl-replay-foundation.sh
test -x tools/run-opengl-production-replay.sh
bash -n tools/run-opengl-production-replay.sh
bash -n tools/check-local-replay-codecs.sh
test -x tools/check-flatpak-replay-codecs.sh
bash -n tools/check-flatpak-replay-codecs.sh
bash -n tools/check-installed-tauri-runtime.sh
test -x install.sh
sh -n install.sh
test -x tools/build-linux-release.sh
bash -n tools/build-linux-release.sh
cargo deny --locked check licenses --hide-inclusion-graph -A license-not-encountered
cargo deny --manifest-path output/tauri-redunar/src-tauri/Cargo.toml \
  --config deny.toml --target x86_64-unknown-linux-gnu --locked \
  check licenses --hide-inclusion-graph -A license-not-encountered
test -x tools/build-tauri-deb.sh
bash -n tools/build-tauri-deb.sh
test -x tools/build-tauri-portable.sh
bash -n tools/build-tauri-portable.sh
test -x tools/build-tauri-arch-package.sh
bash -n tools/build-tauri-arch-package.sh
test -x tools/build-tauri-opensuse-rpm.sh
bash -n tools/build-tauri-opensuse-rpm.sh
test -x tools/sign-release-assets.sh
bash -n tools/sign-release-assets.sh
test -x tools/write-release-version.sh
bash -n tools/write-release-version.sh
test -x tools/build-install-assets.sh
bash -n tools/build-install-assets.sh
test -x tools/render-public-installer.sh
bash -n tools/render-public-installer.sh
test -x tools/test-installer.sh
bash -n tools/test-installer.sh
test -x tools/test-updater-rpm-flow.sh
bash -n tools/test-updater-rpm-flow.sh
test -x tools/test-updater-versioned-rpm.sh
bash -n tools/test-updater-versioned-rpm.sh
tools/test-installer.sh

tools/build-linux-release.sh
python3 tools/generate-license-inventory.py --check
# Session fixtures append random capture directory and socket names. /dev/shm is
# both outside the quota-constrained /tmp and short enough for AF_UNIX paths.
TMPDIR=/dev/shm cargo test --offline \
  --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets --quiet
cargo clippy --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml \
  --all-targets -- -D warnings
node --test output/tauri-redunar/tests/*.test.mjs
node --check output/tauri-redunar/ui/app.js
python3 -m py_compile output/tauri-redunar/build-native.py output/tauri-redunar/install-desktop.py

desktop-file-validate packaging/com.redunar.Redunar.desktop
appstream_status=0
appstreamcli validate --no-net packaging/com.redunar.Redunar.metainfo.xml || appstream_status=$?
if (( appstream_status != 0 && appstream_status != 3 )); then
  exit "$appstream_status"
fi

staging_root="$release_check_root/staging"
tools/stage-tauri-package.sh "$staging_root"
test -x "$staging_root/usr/bin/redunar-tauri"
test -x "$staging_root/usr/bin/redunar-steam-launch"
test -f "$staging_root/usr/bin/libredunar_capture_vulkan.so"
test -f "$staging_root/usr/bin/libredunar_capture_opengl.so"
test -x "$staging_root/usr/libexec/redunar-hotkey-helper"
test -x "$staging_root/usr/libexec/redunar-update-helper"
test -f "$staging_root/usr/share/polkit-1/actions/com.redunar.install-update.policy"
test -f "$staging_root/usr/lib/udev/rules.d/70-redunar-hotkeys.rules"
grep -Fq 'ENV{ID_INPUT_KEYBOARD}=="1"' "$staging_root/usr/lib/udev/rules.d/70-redunar-hotkeys.rules"
grep -Fq 'ENV{ID_INPUT_MOUSE}=="1"' "$staging_root/usr/lib/udev/rules.d/70-redunar-hotkeys.rules"
test -f "$staging_root/usr/share/licenses/redunar/LICENSE"
test -f "$staging_root/usr/share/licenses/redunar/COPYRIGHT"
test -f "$staging_root/usr/share/doc/redunar/DEPENDENCY-LICENSES.md"
test -f "$staging_root/usr/share/doc/redunar/LICENSE-Tauri-API-MIT.txt"
test -f "$staging_root/usr/share/doc/redunar/LICENSE-Tauri-API-APACHE-2.0.txt"
test -f "$staging_root/usr/share/applications/com.redunar.Redunar.desktop"
test -f "$staging_root/usr/share/metainfo/com.redunar.Redunar.metainfo.xml"
test -f "$staging_root/usr/share/icons/hicolor/scalable/apps/com.redunar.Redunar.svg"
test ! -u "$staging_root/usr/libexec/redunar-hotkey-helper"
test ! -g "$staging_root/usr/libexec/redunar-hotkey-helper"
test ! -u "$staging_root/usr/libexec/redunar-update-helper"
test ! -g "$staging_root/usr/libexec/redunar-update-helper"
test ! -e "$staging_root/usr/libexec/redunar-kms-helper"
rg -Fq 'Command::new(pkexec)' output/tauri-redunar/src-tauri/src/updates.rs
! rg -Fq 'restricted helper was not authorized' output/tauri-redunar/src-tauri/src output/tauri-redunar/ui/app.js
rg -Fq 'RedunarService::for_tauri()' output/tauri-redunar/src-tauri/src/backend.rs

installer_output="$release_check_root/install-assets"
mkdir -p "$installer_output"
test_private_key="$release_check_root/release-private.pem"
test_public_key="$release_check_root/release-public.pem"
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$test_private_key" 2>/dev/null
openssl pkey -in "$test_private_key" -pubout -out "$test_public_key"
tools/build-tauri-deb.sh "$installer_output"
tools/build-tauri-portable.sh "$installer_output"
tools/build-tauri-arch-package.sh "$installer_output"
tools/build-tauri-opensuse-rpm.sh "$installer_output"
tools/write-release-version.sh "$installer_output"
test -f "$installer_output/redunar-app-linux-amd64.deb"
test -f "$installer_output/redunar-app-linux-x86_64.tar.gz"
test -f "$installer_output/redunar-app-linux-x86_64.pkg.tar.zst"
test -f "$installer_output/redunar-app-linux-opensuse-x86_64.rpm"
deb_members=$(ar t "$installer_output/redunar-app-linux-amd64.deb")
grep -Fxq debian-binary <<<"$deb_members"
portable_members=$(tar -tzf "$installer_output/redunar-app-linux-x86_64.tar.gz")
grep -Fxq usr/bin/redunar-tauri <<<"$portable_members"
arch_members=$(tar --zstd -tf "$installer_output/redunar-app-linux-x86_64.pkg.tar.zst")
grep -Fxq .PKGINFO <<<"$arch_members"
grep -Fxq usr/bin/redunar-tauri <<<"$arch_members"
tools/sign-release-assets.sh "$installer_output" "$test_private_key" "$test_public_key"
test -s "$installer_output/SHA256SUMS"
test -s "$installer_output/SHA256SUMS.sig"
grep -Eq "^[[:xdigit:]]{64}  VERSION$" "$installer_output/SHA256SUMS"
openssl dgst -sha256 -verify "$test_public_key" \
  -signature "$installer_output/SHA256SUMS.sig" \
  "$installer_output/SHA256SUMS" >/dev/null
tools/render-public-installer.sh example/redunar "$installer_output/install.sh" "$test_public_key"
! grep -Eq '@REDUNAR_(GITHUB_REPOSITORY|RELEASE_PUBLIC_KEY_PEM)@' "$installer_output/install.sh"
REDUNAR_GLIBC_VERSION=2.36 "$installer_output/install.sh" --check \
  | grep -Fq 'https://github.com/example/redunar/releases/latest/download/'

if command -v rpmbuild >/dev/null 2>&1; then
  rpm_output=$(mktemp -d)
  tools/build-local-tauri-rpm.sh "$rpm_output"
  rpm_path=$(find "$rpm_output" -type f \( -name 'redunar-app.rpm' -o -name 'redunar-app-*.rpm' \) -print -quit)
  test -n "$rpm_path"
  test "$(rpm -qp --qf '%{NAME}' "$rpm_path")" = redunar-app
  # Both FFmpeg package families provide these files. The full codec library
  # and standalone freeworld package provide the same architecture capability.
  rpm_requirements=$(rpm -qp --requires "$rpm_path")
  grep -Fxq '/usr/bin/ffmpeg' <<<"$rpm_requirements"
  grep -Fxq '/usr/bin/ffprobe' <<<"$rpm_requirements"
  grep -Fxq 'libavcodec-freeworld(x86-64)' <<<"$rpm_requirements"
  grep -Fxq '/usr/lib64/gstreamer-1.0/libgstlibav.so' <<<"$rpm_requirements"
  ! grep -Eq '^ffmpeg(-free|-libs)?([[:space:]]|$)' <<<"$rpm_requirements"
  ! rpm -qp --qf '[%{CONFLICTNAME}\n][%{OBSOLETENAME}\n]' "$rpm_path" | grep -Eiq 'ffmpeg|libavcodec'
  rpm_scripts=$(rpm -qp --scripts "$rpm_path")
  grep -Fq 'postinstall scriptlet' <<<"$rpm_scripts"
  grep -Fq '/usr/bin/udevadm control --reload-rules' <<<"$rpm_scripts"
  grep -Fq '/usr/bin/udevadm trigger --subsystem-match=input --sysname-match=event* --property-match=ID_INPUT_KEYBOARD=1 --action=change' <<<"$rpm_scripts"
  grep -Fq '/usr/bin/udevadm trigger --subsystem-match=input --sysname-match=event* --property-match=ID_INPUT_MOUSE=1 --action=change' <<<"$rpm_scripts"
  ! grep -Eiq 'pkexec|polkit|steam|/usr/bin/sudo' <<<"$rpm_scripts"
fi

printf '%s\n' 'Redunar Tauri release checks passed.'
