#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p "$workspace_root/target"
test_root=$(mktemp -d "$workspace_root/target/.installer-tests.XXXXXX")
trap 'rm -rf -- "$test_root"' EXIT

write_os_release() {
  local name=$1
  local id=$2
  local version=$3
  local like=${4:-}
  cat >"$test_root/$name" <<EOF
ID=$id
VERSION_ID="$version"
ID_LIKE="$like"
EOF
}

write_os_release fedora fedora 44
write_os_release ubuntu ubuntu 24.04 debian
write_os_release debian-old debian 12
write_os_release arch arch rolling
write_os_release opensuse opensuse-tumbleweed 20260914 'suse opensuse'
write_os_release bazzite bazzite 44 fedora
write_os_release alpine alpine 3.23
write_os_release nixos nixos 26.05

test_private_key="$test_root/release-private.pem"
test_public_key="$test_root/release-public.pem"
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$test_private_key" 2>/dev/null
openssl pkey -in "$test_private_key" -pubout -out "$test_public_key"

plan() {
  local fixture=$1
  shift
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
    REDUNAR_OS_RELEASE_FILE="$test_root/$fixture" \
    REDUNAR_ARCHITECTURE=x86_64 \
    REDUNAR_GLIBC_VERSION=2.41 \
    "$workspace_root/install.sh" --check "$@"
}

expect_failure() {
  local expected=$1
  shift
  local output
  if output=$("$@" 2>&1); then
    printf 'expected installer failure containing: %s\n' "$expected" >&2
    exit 1
  fi
  grep -Fq "$expected" <<<"$output" || {
    printf 'installer failure did not contain %s:\n%s\n' "$expected" "$output" >&2
    exit 1
  }
}

fedora_plan=$(plan fedora)
grep -Fq 'redunar-app-linux-x86_64.rpm' <<<"$fedora_plan"
grep -Fq 'dnf install -y' <<<"$fedora_plan"

ubuntu_plan=$(plan ubuntu)
grep -Fq 'redunar-app-linux-amd64.deb' <<<"$ubuntu_plan"
grep -Fq 'apt-get install -y' <<<"$ubuntu_plan"

arch_plan=$(plan arch)
grep -Fq 'redunar-app-linux-x86_64.pkg.tar.zst' <<<"$arch_plan"
grep -Fq 'native package ownership' <<<"$arch_plan"

suse_plan=$(plan opensuse)
grep -Fq 'redunar-app-linux-opensuse-x86_64.rpm' <<<"$suse_plan"
grep -Fq 'zypper --non-interactive install' <<<"$suse_plan"

version_plan=$(plan ubuntu --version v0.1.0)
grep -Fq '/releases/download/v0.1.0/redunar-app-linux-amd64.deb' <<<"$version_plan"

expect_failure 'glibc 2.35 is too old' env \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_OS_RELEASE_FILE="$test_root/debian-old" \
  REDUNAR_ARCHITECTURE=x86_64 REDUNAR_GLIBC_VERSION=2.35 \
  "$workspace_root/install.sh" --check
expect_failure 'does not support immutable distribution bazzite' env \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_OS_RELEASE_FILE="$test_root/bazzite" \
  REDUNAR_ARCHITECTURE=x86_64 REDUNAR_GLIBC_VERSION=2.41 \
  "$workspace_root/install.sh" --check
expect_failure 'cannot be installed on Alpine/musl' env \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_OS_RELEASE_FILE="$test_root/alpine" \
  REDUNAR_ARCHITECTURE=x86_64 REDUNAR_GLIBC_VERSION=2.41 \
  "$workspace_root/install.sh" --check
expect_failure 'does not support immutable distribution nixos' env \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_OS_RELEASE_FILE="$test_root/nixos" \
  REDUNAR_ARCHITECTURE=x86_64 REDUNAR_GLIBC_VERSION=2.41 \
  "$workspace_root/install.sh" --check
expect_failure 'unsupported architecture: aarch64' env \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_OS_RELEASE_FILE="$test_root/fedora" \
  REDUNAR_ARCHITECTURE=aarch64 REDUNAR_GLIBC_VERSION=2.41 \
  "$workspace_root/install.sh" --check
expect_failure 'GitHub repository is not configured yet' \
  "$workspace_root/install.sh" --check

download_root="$test_root/downloads"
fake_bin="$test_root/fake-bin"
installer_tmp="$test_root/installer-tmp"
mkdir -p "$download_root" "$fake_bin" "$installer_tmp"
printf '%s\n' 'isolated DEB fixture' >"$download_root/redunar-app-linux-amd64.deb"
(
  cd "$download_root"
  sha256sum redunar-app-linux-amd64.deb >SHA256SUMS
)
openssl dgst -sha256 -sign "$test_private_key" \
  -out "$download_root/SHA256SUMS.sig" "$download_root/SHA256SUMS"
cat >"$fake_bin/id" <<'EOF'
#!/bin/sh
if [ "${1:-}" = -u ]; then
  printf '%s\n' 0
else
  exec /usr/bin/id "$@"
fi
EOF
cat >"$fake_bin/apt-get" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$REDUNAR_TEST_COMMAND_LOG"
EOF
chmod 0755 "$fake_bin/id" "$fake_bin/apt-get"

"$workspace_root/tools/render-public-installer.sh" \
  example/redunar "$test_root/rendered-install.sh" "$test_public_key" >/dev/null

command_log="$test_root/package-manager.log"
PATH="$fake_bin:$PATH" \
TMPDIR="$installer_tmp" \
REDUNAR_TEST_COMMAND_LOG="$command_log" \
REDUNAR_RELEASE_BASE_URL="file://$download_root" \
REDUNAR_ALLOW_INSECURE_URL=1 \
REDUNAR_OS_RELEASE_FILE="$test_root/ubuntu" \
REDUNAR_ARCHITECTURE=x86_64 \
REDUNAR_GLIBC_VERSION=2.41 \
  "$test_root/rendered-install.sh" \
  >"$test_root/install-output.log" 2>"$test_root/install-error.log"
test ! -s "$test_root/install-error.log"
grep -Fxq update "$command_log"
grep -Eq '^install -y .*/redunar-app-linux-amd64\.deb$' "$command_log"
grep -Fq 'Redunar installed successfully.' "$test_root/install-output.log"

printf '%s\n' 'corrupted DEB fixture' >"$download_root/redunar-app-linux-amd64.deb"
expect_failure 'downloaded release checksum does not match' env \
  PATH="$fake_bin:$PATH" TMPDIR="$installer_tmp" \
  REDUNAR_TEST_COMMAND_LOG="$command_log" \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_RELEASE_PUBLIC_KEY_FILE="$test_public_key" \
  REDUNAR_RELEASE_BASE_URL="file://$download_root" \
  REDUNAR_ALLOW_INSECURE_URL=1 \
  REDUNAR_OS_RELEASE_FILE="$test_root/ubuntu" \
  REDUNAR_ARCHITECTURE=x86_64 REDUNAR_GLIBC_VERSION=2.41 \
  "$workspace_root/install.sh"

printf '%s\n' 'invalid signature' >"$download_root/SHA256SUMS.sig"
expect_failure 'release manifest signature is invalid' env \
  PATH="$fake_bin:$PATH" TMPDIR="$installer_tmp" \
  REDUNAR_TEST_COMMAND_LOG="$command_log" \
  REDUNAR_GITHUB_REPOSITORY=example/redunar \
  REDUNAR_RELEASE_PUBLIC_KEY_FILE="$test_public_key" \
  REDUNAR_RELEASE_BASE_URL="file://$download_root" \
  REDUNAR_ALLOW_INSECURE_URL=1 \
  REDUNAR_OS_RELEASE_FILE="$test_root/ubuntu" \
  REDUNAR_ARCHITECTURE=x86_64 REDUNAR_GLIBC_VERSION=2.41 \
  "$workspace_root/install.sh"

printf '%s\n' 'Redunar installer tests passed.'
