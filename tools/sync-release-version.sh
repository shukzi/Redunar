#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=${1:-}
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf '%s\n' 'usage: tools/sync-release-version.sh MAJOR.MINOR.PATCH' >&2
  exit 2
fi

python3 - "$workspace_root" "$version" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
version = sys.argv[2]

def replace_once(path: pathlib.Path, pattern: str, replacement: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"could not update exactly one version in {path}")
    path.write_text(updated, encoding="utf-8")

replace_once(
    root / "packaging/redunar-app.spec",
    r"^(Version:\s+)[0-9]+\.[0-9]+\.[0-9]+$",
    rf"\g<1>{version}",
)
replace_once(
    root / "output/tauri-redunar/src-tauri/Cargo.toml",
    r'^(version\s*=\s*")[0-9]+\.[0-9]+\.[0-9]+("\s*)$',
    rf"\g<1>{version}\g<2>",
)
replace_once(
    root / "output/tauri-redunar/src-tauri/tauri.conf.json",
    r'^(\s*"version"\s*:\s*")[0-9]+\.[0-9]+\.[0-9]+(",\s*)$',
    rf"\g<1>{version}\g<2>",
)

metainfo = root / "packaging/com.redunar.Redunar.metainfo.xml"
replace_once(
    metainfo,
    r'(<release\s+version=")[0-9]+\.[0-9]+\.[0-9]+("\s+date=")',
    rf"\g<1>{version}\g<2>",
)

print(f"synchronized release-facing metadata to {version}")
PY
