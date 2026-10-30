#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
asset_directory=${1:-}
if [[ "$asset_directory" != /* || ! -d "$asset_directory" ]]; then
  printf '%s\n' 'usage: tools/write-release-version.sh /absolute/asset/directory' >&2
  exit 2
fi

package_version=$(awk '$1 == "Version:" { print $2; exit }' "$workspace_root/packaging/redunar-app.spec")
app_version=$(awk -F '"' '/^version = "/ { print $2; exit }' "$workspace_root/output/tauri-redunar/src-tauri/Cargo.toml")
if [[ ! $package_version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || $package_version != "$app_version" ]]; then
  printf 'Redunar release versions disagree: package=%s app=%s\n' "$package_version" "$app_version" >&2
  exit 1
fi
printf '%s\n' "$package_version" >"$asset_directory/VERSION"
