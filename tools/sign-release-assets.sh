#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
asset_directory=${1:-}
private_key=${2:-${REDUNAR_RELEASE_SIGNING_KEY:-}}
public_key=${3:-"$workspace_root/packaging/release-signing-public.pem"}

if [[ "$asset_directory" != /* || ! -d "$asset_directory" || -z "$private_key" ]]; then
  printf '%s\n' 'usage: tools/sign-release-assets.sh /absolute/asset/directory /absolute/private-key.pem [public-key.pem]' >&2
  exit 2
fi
if [[ "$private_key" != /* || ! -r "$private_key" || ! -r "$public_key" ]]; then
  printf '%s\n' 'release signing requires readable private and public key files' >&2
  exit 2
fi
command -v openssl >/dev/null 2>&1 || {
  printf '%s\n' 'OpenSSL is required to sign release assets' >&2
  exit 1
}

private_public_key="$asset_directory/.release-public-key.pem"
trap 'rm -f -- "$private_public_key"' EXIT
openssl pkey -in "$private_key" -pubout -out "$private_public_key"
if ! cmp -s "$private_public_key" "$public_key"; then
  printf '%s\n' 'the release private key does not match the configured public key' >&2
  exit 1
fi

mapfile -d '' artifacts < <(
  find "$asset_directory" -maxdepth 1 -type f -name 'redunar-app-linux-*' \
    ! -name '*.sha256' -print0 | sort -z
)
if (( ${#artifacts[@]} == 0 )); then
  printf '%s\n' 'no Redunar release assets were found to sign' >&2
  exit 1
fi

manifest="$asset_directory/SHA256SUMS"
signature="$asset_directory/SHA256SUMS.sig"
rm -f -- "$manifest" "$signature"
for artifact in "${artifacts[@]}"; do
  (
    cd "$asset_directory"
    sha256sum "$(basename -- "$artifact")"
  ) >>"$manifest"
done
openssl dgst -sha256 -sign "$private_key" -out "$signature" "$manifest"
openssl dgst -sha256 -verify "$public_key" -signature "$signature" "$manifest" >/dev/null

printf 'Signed %d Redunar release assets: %s\n' "${#artifacts[@]}" "$manifest"
