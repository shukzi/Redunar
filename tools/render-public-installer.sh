#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
repository=${1:-}
destination=${2:-}
public_key=${3:-"$workspace_root/packaging/release-signing-public.pem"}

if [[ ! "$repository" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]]; then
  printf '%s\n' 'usage: tools/render-public-installer.sh owner/repository /absolute/output/install.sh' >&2
  exit 2
fi
if [[ "$destination" != /* || "$destination" == / ]]; then
  printf '%s\n' 'installer destination must be an absolute file path' >&2
  exit 2
fi
if [[ ! -r "$public_key" ]]; then
  printf 'release public key cannot be read: %s\n' "$public_key" >&2
  exit 2
fi
openssl pkey -pubin -in "$public_key" -noout >/dev/null

mkdir -p "$(dirname -- "$destination")"
python3 - "$workspace_root/install.sh" "$repository" "$public_key" "$destination" <<'PY'
from pathlib import Path
import sys

template_path, repository, public_key_path, destination_path = sys.argv[1:]
template = Path(template_path).read_text()
public_key = Path(public_key_path).read_text().rstrip("\n")
rendered = template.replace("@REDUNAR_GITHUB_REPOSITORY@", repository)
rendered = rendered.replace("@REDUNAR_RELEASE_PUBLIC_KEY_PEM@", public_key)
Path(destination_path).write_text(rendered)
PY
chmod 0755 "$destination"

if grep -Eq '@REDUNAR_(GITHUB_REPOSITORY|RELEASE_PUBLIC_KEY_PEM)@' "$destination"; then
  printf '%s\n' 'rendered installer still contains a publication placeholder' >&2
  exit 1
fi
sh -n "$destination"
printf 'Rendered public installer: %s\n' "$destination"
