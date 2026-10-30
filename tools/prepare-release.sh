#!/usr/bin/env bash
# Prepare unsigned packages before any signing or GitHub publication.
set -euo pipefail
workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$workspace_root"
tag=${1:-}
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf '%s\n' 'usage: tools/prepare-release.sh vMAJOR.MINOR.PATCH' >&2; exit 2
fi
if [[ -n $(git status --porcelain) ]]; then
  printf '%s\n' 'Release candidates require a clean, committed source tree.' >&2; exit 1
fi
commit=$(git rev-parse HEAD)
if [[ $(git rev-parse "$tag^{commit}") != "$commit" ]]; then
  printf '%s\n' 'The release tag must point to the checked-out commit.' >&2; exit 1
fi
assets="$workspace_root/.redunar-build/candidates/$tag-$commit"
if [[ -e "$assets" ]]; then
  printf '%s\n' 'This candidate already exists; retain it and use a new candidate commit/version.' >&2; exit 1
fi
mkdir -p "$workspace_root/.redunar-build"
exec 8>"$workspace_root/.redunar-build/candidate.lock"
flock -n 8 || { printf '%s\n' 'Another release candidate is being prepared.' >&2; exit 1; }
tools/check-release-inputs.sh "$tag"
tools/validate.sh release
source "$workspace_root/tools/lib/release-paths.sh"
lock_release_artifacts
verify_release_artifacts
mkdir -p "$assets"
complete=0
trap 'if (( complete == 0 )); then printf "%s\n" "INCOMPLETE candidate: $assets" >&2; fi' EXIT
tools/build-tauri-deb.sh "$assets"
tools/build-tauri-arch-package.sh "$assets"
tools/build-tauri-opensuse-rpm.sh "$assets"
tools/build-local-tauri-rpm.sh "$assets/rpm"
mv "$assets/rpm/redunar-app.rpm" "$assets/redunar-app-linux-x86_64.rpm"
rmdir "$assets/rpm"
tools/build-tauri-portable.sh "$assets"
tools/write-release-version.sh "$assets"
for asset in "$assets"/redunar-app-linux-*; do
  (cd "$assets"; sha256sum "$(basename -- "$asset")" >"$(basename -- "$asset").sha256")
done
cp .redunar-build/linux/source-manifest.json .redunar-build/linux/linux-release-baseline.txt "$assets/"
printf '%s\n' "$commit" >"$assets/SOURCE_COMMIT"
printf '%s\n' "$tag" >"$assets/RELEASE_TAG"
if [[ $(git rev-parse HEAD) != "$commit" || -n $(git status --porcelain) ]]; then
  printf '%s\n' 'Source changed during qualification; candidate is incomplete.' >&2; exit 1
fi
complete=1
printf 'Unsigned candidate ready: %s\n' "$assets"
