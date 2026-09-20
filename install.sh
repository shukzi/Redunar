#!/bin/sh
# Redunar release installer template.
#
# Before publication, render the GitHub repository placeholder with
# tools/render-public-installer.sh. For local checks it can be supplied through
# REDUNAR_GITHUB_REPOSITORY=owner/repository.

set -eu

umask 022

program_name=redunar-install
configured_github_repository='@REDUNAR_GITHUB_REPOSITORY@'
minimum_glibc=2.36
dry_run=0
release_tag=${REDUNAR_VERSION:-latest}
os_release_file=${REDUNAR_OS_RELEASE_FILE:-/etc/os-release}
architecture=${REDUNAR_ARCHITECTURE:-$(uname -m)}

say() {
  printf '%s\n' "$*"
}

fail() {
  printf '%s: %s\n' "$program_name" "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Install the latest Redunar release:
  curl --proto '=https' --tlsv1.2 -fsSL https://redunar.com/install.sh | sh

Options when running a downloaded copy:
  --check          Print the detected installation plan without changing anything.
  --version TAG    Install a specific GitHub release tag instead of the latest.
  --help           Show this help.

Release preparation override:
  REDUNAR_GITHUB_REPOSITORY=owner/repository
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --check|--dry-run)
      dry_run=1
      ;;
    --version)
      [ "$#" -ge 2 ] || fail '--version requires a release tag'
      release_tag=$2
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
  shift
done

github_repository=${REDUNAR_GITHUB_REPOSITORY:-$configured_github_repository}
case "$github_repository" in
  ''|@*)
    fail 'the GitHub repository is not configured yet; render this installer before publication'
    ;;
  */*) ;;
  *) fail 'REDUNAR_GITHUB_REPOSITORY must use owner/repository format' ;;
esac
case "$github_repository" in
  *[!A-Za-z0-9._/-]*|*//*|/*|*/|*/*/*)
    fail 'the GitHub repository contains unsupported characters'
    ;;
esac
case "$release_tag" in
  ''|*[!A-Za-z0-9._-]*) fail 'the release tag contains unsupported characters' ;;
esac

read_os_value() {
  key=$1
  value=$(awk -F= -v wanted="$key" '$1 == wanted { sub(/^[^=]*=/, ""); print; exit }' "$os_release_file")
  case "$value" in
    \"*\") value=${value#\"}; value=${value%\"} ;;
    \'*\') value=${value#\'}; value=${value%\'} ;;
  esac
  printf '%s' "$value"
}

[ -r "$os_release_file" ] || fail "cannot read distribution information from $os_release_file"
distribution_id=$(read_os_value ID)
distribution_like=$(read_os_value ID_LIKE)
distribution_version=$(read_os_value VERSION_ID)
[ -n "$distribution_id" ] || fail "distribution ID is missing from $os_release_file"

case "$architecture" in
  x86_64|amd64) architecture=x86_64 ;;
  *) fail "unsupported architecture: $architecture; current releases require x86_64" ;;
esac

if [ -e /run/ostree-booted ] || [ "$distribution_id" = bazzite ] || \
   [ "$distribution_id" = steamos ] || [ "$distribution_id" = nixos ]; then
  fail "the current installer does not support immutable distribution $distribution_id"
fi
if [ "$distribution_id" = alpine ]; then
  fail 'the current release uses glibc and cannot be installed on Alpine/musl'
fi

distribution_words=" $distribution_id $distribution_like "
case "$distribution_words" in
  *' ubuntu '*|*' debian '*|*' linuxmint '*|*' pop '*)
    distribution_family=debian
    package_kind=deb
    asset_name=redunar-app-linux-amd64.deb
    ;;
  *' fedora '*|*' rhel '*|*' centos '*|*' rocky '*|*' almalinux '*|*' nobara '*)
    distribution_family=fedora
    package_kind=rpm
    asset_name=redunar-app-linux-x86_64.rpm
    ;;
  *' arch '*|*' manjaro '*|*' endeavouros '*|*' cachyos '*)
    distribution_family=arch
    package_kind=arch
    asset_name=redunar-app-linux-x86_64.pkg.tar.zst
    ;;
  *' opensuse '*|*' opensuse-tumbleweed '*|*' opensuse-leap '*|*' sles '*|*' suse '*)
    distribution_family=suse
    package_kind=rpm
    asset_name=redunar-app-linux-opensuse-x86_64.rpm
    ;;
  *)
    fail "unsupported distribution: $distribution_id ${distribution_version:-unknown}"
    ;;
esac

glibc_version=${REDUNAR_GLIBC_VERSION:-}
if [ -z "$glibc_version" ] && command -v getconf >/dev/null 2>&1; then
  glibc_version=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')
fi
if [ -z "$glibc_version" ]; then
  fail 'could not determine the system glibc version'
fi

version_at_least() {
  awk -v have="$1" -v need="$2" 'BEGIN {
    split(have, h, "."); split(need, n, ".");
    for (i = 1; i <= 3; i++) {
      hv = h[i] + 0; nv = n[i] + 0;
      if (hv > nv) exit 0;
      if (hv < nv) exit 1;
    }
    exit 0;
  }'
}

if ! version_at_least "$glibc_version" "$minimum_glibc"; then
  fail "glibc $glibc_version is too old; this release requires glibc $minimum_glibc or newer"
fi

if [ -n "${REDUNAR_RELEASE_BASE_URL:-}" ]; then
  release_base_url=${REDUNAR_RELEASE_BASE_URL%/}
elif [ "$release_tag" = latest ]; then
  release_base_url="https://github.com/$github_repository/releases/latest/download"
else
  release_base_url="https://github.com/$github_repository/releases/download/$release_tag"
fi
case "$release_base_url" in
  https://*) ;;
  *)
    [ "${REDUNAR_ALLOW_INSECURE_URL:-0}" = 1 ] || fail 'release downloads must use HTTPS'
    ;;
esac

asset_url="$release_base_url/$asset_name"

say "Redunar installation plan"
say "  Distribution: $distribution_id ${distribution_version:-unknown} ($distribution_family)"
say "  Architecture: $architecture"
say "  glibc: $glibc_version"
say "  Release: $release_tag"
say "  Asset: $asset_url"

if [ "$dry_run" -eq 1 ]; then
  case "$distribution_family" in
    debian) say '  Installer: apt-get install -y <downloaded DEB>' ;;
    fedora) say '  Installer: dnf install -y <downloaded RPM>' ;;
    arch) say '  Installer: pacman dependencies plus native package ownership' ;;
    suse) say '  Installer: zypper --non-interactive install <downloaded openSUSE RPM>' ;;
  esac
  say 'No changes made.'
  exit 0
fi

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/redunar-install.XXXXXX") || fail 'could not create a temporary directory'
cleanup() {
  rm -rf -- "$temporary_directory"
}
trap cleanup EXIT HUP INT TERM

download() {
  source_url=$1
  destination=$2
  if command -v curl >/dev/null 2>&1; then
    if [ "${REDUNAR_ALLOW_INSECURE_URL:-0}" = 1 ]; then
      curl -fsSL --retry 3 --connect-timeout 20 -o "$destination" "$source_url"
    else
      curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 20 -o "$destination" "$source_url"
    fi
  elif command -v wget >/dev/null 2>&1; then
    wget -q --https-only -O "$destination" "$source_url"
  else
    fail 'curl or wget is required to download Redunar'
  fi
}

asset_path="$temporary_directory/$asset_name"
manifest_path="$temporary_directory/SHA256SUMS"
signature_path="$temporary_directory/SHA256SUMS.sig"
public_key_path="$temporary_directory/redunar-release-public.pem"
say "Downloading $asset_name..."
download "$asset_url" "$asset_path"
download "$release_base_url/SHA256SUMS" "$manifest_path"
download "$release_base_url/SHA256SUMS.sig" "$signature_path"

if ! command -v openssl >/dev/null 2>&1; then
  fail 'OpenSSL is required to authenticate the Redunar release manifest'
fi
if [ -n "${REDUNAR_RELEASE_PUBLIC_KEY_FILE:-}" ]; then
  [ -r "$REDUNAR_RELEASE_PUBLIC_KEY_FILE" ] || fail 'the configured release public key cannot be read'
  cp "$REDUNAR_RELEASE_PUBLIC_KEY_FILE" "$public_key_path"
else
  cat >"$public_key_path" <<'REDUNAR_RELEASE_PUBLIC_KEY'
@REDUNAR_RELEASE_PUBLIC_KEY_PEM@
REDUNAR_RELEASE_PUBLIC_KEY
  if grep -Fq '@REDUNAR_RELEASE_''PUBLIC_KEY_PEM@' "$public_key_path"; then
    fail 'the release public key is not configured; render this installer before publication'
  fi
fi
if ! openssl dgst -sha256 -verify "$public_key_path" \
  -signature "$signature_path" "$manifest_path" >/dev/null 2>&1; then
  fail 'the Redunar release manifest signature is invalid'
fi

expected_checksum=$(awk -v wanted="$asset_name" '
  $2 == wanted || $2 == "*" wanted { checksum = $1; matches++ }
  END { if (matches == 1) print checksum; else exit 1 }
' "$manifest_path") || fail 'the signed release manifest does not contain exactly one checksum for this asset'
case "$expected_checksum" in
  ''|*[!A-Fa-f0-9]*) fail 'the release checksum is malformed' ;;
esac
[ "${#expected_checksum}" -eq 64 ] || fail 'the release checksum is malformed'
if command -v sha256sum >/dev/null 2>&1; then
  actual_checksum=$(sha256sum "$asset_path" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  actual_checksum=$(shasum -a 256 "$asset_path" | awk '{print $1}')
else
  fail 'sha256sum or shasum is required to verify the download'
fi
[ "$actual_checksum" = "$expected_checksum" ] || fail 'the downloaded release checksum does not match'

run_as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  elif command -v doas >/dev/null 2>&1; then
    doas "$@"
  else
    fail 'root access through sudo or doas is required to install Redunar'
  fi
}

install_portable_payload() {
  payload_directory="$temporary_directory/payload"
  mkdir -p "$payload_directory"
  if ! tar -tzf "$asset_path" | awk '
    /^usr\/(bin|lib|libexec|share)\// { next }
    /^usr\/?$/ { next }
    { exit 1 }
  '; then
    fail 'the portable release contains an unexpected path'
  fi
  tar -xzf "$asset_path" --no-same-owner -C "$payload_directory"

  for required_file in \
    usr/bin/redunar-tauri \
    usr/bin/redunar-steam-launch \
    usr/bin/libredunar_capture_vulkan.so \
    usr/libexec/redunar-hotkey-helper \
    usr/lib/udev/rules.d/70-redunar-hotkeys.rules \
    usr/share/applications/com.redunar.Redunar.desktop \
    usr/share/metainfo/com.redunar.Redunar.metainfo.xml \
    usr/share/licenses/redunar/LICENSE \
    usr/share/licenses/redunar/COPYRIGHT
  do
    [ -f "$payload_directory/$required_file" ] || fail "portable release is missing $required_file"
  done

  run_as_root install -d -m 0755 \
    /usr/bin /usr/libexec /usr/lib/udev/rules.d \
    /usr/share/applications /usr/share/metainfo \
    /usr/share/icons/hicolor/scalable/apps /usr/share/doc/redunar \
    /usr/share/licenses/redunar
  for icon_size in 16 32 48 64 128 256 512 1024; do
    run_as_root install -d -m 0755 "/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps"
  done
  run_as_root install -m 0755 "$payload_directory/usr/bin/redunar-tauri" /usr/bin/redunar-tauri
  run_as_root install -m 0755 "$payload_directory/usr/bin/redunar-steam-launch" /usr/bin/redunar-steam-launch
  run_as_root install -m 0755 "$payload_directory/usr/bin/libredunar_capture_vulkan.so" /usr/bin/libredunar_capture_vulkan.so
  run_as_root install -m 0755 "$payload_directory/usr/libexec/redunar-hotkey-helper" /usr/libexec/redunar-hotkey-helper
  run_as_root install -m 0644 "$payload_directory/usr/lib/udev/rules.d/70-redunar-hotkeys.rules" /usr/lib/udev/rules.d/70-redunar-hotkeys.rules
  run_as_root install -m 0644 "$payload_directory/usr/share/applications/com.redunar.Redunar.desktop" /usr/share/applications/com.redunar.Redunar.desktop
  run_as_root install -m 0644 "$payload_directory/usr/share/metainfo/com.redunar.Redunar.metainfo.xml" /usr/share/metainfo/com.redunar.Redunar.metainfo.xml
  run_as_root install -m 0644 "$payload_directory/usr/share/icons/hicolor/scalable/apps/com.redunar.Redunar.svg" /usr/share/icons/hicolor/scalable/apps/com.redunar.Redunar.svg
  for icon_size in 16 32 48 64 128 256 512 1024; do
    run_as_root install -m 0644 \
      "$payload_directory/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps/com.redunar.Redunar.png" \
      "/usr/share/icons/hicolor/${icon_size}x${icon_size}/apps/com.redunar.Redunar.png"
  done
  for notice in \
    LICENSE-Lucide.txt \
    LICENSE-Noto-Sans-Mono.txt \
    LICENSE-libc-APACHE.txt \
    LICENSE-libc-MIT.txt \
    LICENSE-Tauri-API-APACHE-2.0.txt \
    LICENSE-Tauri-API-MIT.txt \
    DEPENDENCY-LICENSES.md \
    THIRD-PARTY-NOTICES.md
  do
    run_as_root install -m 0644 \
      "$payload_directory/usr/share/doc/redunar/$notice" \
      "/usr/share/doc/redunar/$notice"
  done
  run_as_root install -m 0644 \
    "$payload_directory/usr/share/licenses/redunar/LICENSE" \
    /usr/share/licenses/redunar/LICENSE
  run_as_root install -m 0644 \
    "$payload_directory/usr/share/licenses/redunar/COPYRIGHT" \
    /usr/share/licenses/redunar/COPYRIGHT
  if command -v udevadm >/dev/null 2>&1; then
    run_as_root udevadm control --reload-rules || :
    run_as_root udevadm trigger --subsystem-match=input --sysname-match='event*' --property-match=ID_INPUT_KEYBOARD=1 --action=change || :
  fi
}

case "$distribution_family" in
  debian)
    command -v apt-get >/dev/null 2>&1 || fail 'apt-get is required on this distribution'
    run_as_root env DEBIAN_FRONTEND=noninteractive apt-get update
    run_as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "$asset_path"
    ;;
  fedora)
    if command -v dnf >/dev/null 2>&1; then
      run_as_root dnf install -y "$asset_path"
    elif command -v yum >/dev/null 2>&1; then
      run_as_root yum localinstall -y "$asset_path"
    else
      fail 'dnf or yum is required on this distribution'
    fi
    ;;
  arch)
    command -v pacman >/dev/null 2>&1 || fail 'pacman is required on this distribution'
    audio_provider=()
    if ! pacman -Q pipewire-pulse >/dev/null 2>&1 && \
       ! pacman -Q pulseaudio >/dev/null 2>&1; then
      audio_provider=(pipewire-pulse)
    fi
    run_as_root pacman -S --needed --noconfirm \
      gtk3 webkit2gtk-4.1 libdrm libpulse opus ffmpeg gst-libav "${audio_provider[@]}"
    run_as_root pacman -U --needed --noconfirm "$asset_path"
    ;;
  suse)
    command -v zypper >/dev/null 2>&1 || fail 'zypper is required on this distribution'
    run_as_root zypper --non-interactive install --no-recommends "$asset_path"
    ;;
esac

say 'Redunar installed successfully. Start it from the application menu or run redunar-tauri.'
