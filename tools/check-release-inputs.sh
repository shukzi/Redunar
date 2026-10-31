#!/usr/bin/env bash
set -euo pipefail

release_tag=${1:-}
workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ ! "$release_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf '%s\n' 'release tag must use vMAJOR.MINOR.PATCH' >&2
  exit 1
fi

release_version=${release_tag#v}
spec_version=$(awk '$1 == "Version:" { print $2; exit }' "$workspace_root/packaging/redunar-app.spec")
tauri_version=$(awk -F'"' '$1 ~ /^version[[:space:]]*=/ { print $2; exit }' "$workspace_root/output/tauri-redunar/src-tauri/Cargo.toml")
tauri_config_version=$(python3 - "$workspace_root/output/tauri-redunar/src-tauri/tauri.conf.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as config_file:
    version = json.load(config_file)["version"]
if not isinstance(version, str):
    raise SystemExit("Tauri config version must be a string")
print(version)
PY
)
metainfo_version=$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' "$workspace_root/packaging/com.redunar.Redunar.metainfo.xml" | head -n 1)

for component in spec_version tauri_version tauri_config_version metainfo_version; do
  if [[ "${!component}" != "$release_version" ]]; then
    printf 'release version mismatch: tag=%s %s=%s; run tools/sync-release-version.sh %s\n' \
      "$release_version" "${component%_version}" "${!component}" "$release_version" >&2
    exit 1
  fi
done

printf 'release inputs match %s\n' "$release_tag"
