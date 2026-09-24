#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
image=registry.fedoraproject.org/fedora:44
old_package="$workspace_root/target/packages/redunar-app.rpm"
for command_name in rsync cargo npm rpmbuild rpm rpm2cpio cpio podman sha256sum; do
  command -v "$command_name" >/dev/null || {
    printf 'Missing command: %s\n' "$command_name" >&2
    exit 2
  }
done
if [[ ! -f $old_package ]] || ! podman image exists "$image"; then
  printf '%s\n' 'A local Redunar RPM and cached Fedora 44 image are required; no image will be pulled.' >&2
  exit 2
fi
old_version=$(rpm -qp --qf '%{VERSION}' "$old_package")
if [[ ! $old_version =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  printf 'Unexpected RPM version: %s\n' "$old_version" >&2
  exit 1
fi
new_version="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.$((10#${BASH_REMATCH[3]} + 1))"

# Build against an isolated snapshot so the installed app, working tree, and
# ordinary release artifacts retain their current version and contents.
fixture=$(mktemp -d "$workspace_root/target/.updater-versioned.XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
snapshot="$fixture/source"
mkdir -p "$snapshot" "$fixture/packages" "$fixture/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS} "$fixture/tmp"
rsync -a --exclude='.git' --exclude='target' --exclude='node_modules' \
  --exclude='dist' "$workspace_root/" "$snapshot/"
cp -a --reflink=auto "$(readlink -f "$workspace_root/output/tauri-redunar/src-tauri/target")" \
  "$fixture/tauri-target"
ln -s "$fixture/tauri-target" "$snapshot/output/tauri-redunar/src-tauri/target"
ln -s "$(readlink -f "$workspace_root/output/tauri-redunar/node_modules")" \
  "$snapshot/output/tauri-redunar/node_modules"
rm "$snapshot/output/tauri-redunar/src-tauri/gen"
cp -a --reflink=auto "$(readlink -f "$workspace_root/output/tauri-redunar/src-tauri/gen")" \
  "$snapshot/output/tauri-redunar/src-tauri/gen"

python3 - "$snapshot" "$old_version" "$new_version" <<'PY'
from pathlib import Path
import sys

root, old, new = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
for relative, before, after in (
    ('output/tauri-redunar/src-tauri/Cargo.toml', f'version = "{old}"', f'version = "{new}"'),
    ('output/tauri-redunar/src-tauri/tauri.conf.json', f'"version": "{old}"', f'"version": "{new}"'),
    ('packaging/redunar-app.spec', f'Version:        {old}', f'Version:        {new}'),
    ('output/tauri-redunar/ui/index.html', '__REDUNAR_VERSION__', new),
):
    path = root / relative
    contents = path.read_text()
    if contents.count(before) != 1:
        raise SystemExit(f'Expected one version marker in {relative}')
    path.write_text(contents.replace(before, after, 1))
PY

ui="$snapshot/output/tauri-redunar"
(cd "$ui" && npm run build >"$fixture/ui-build.log" 2>&1)
CARGO_TARGET_DIR="$fixture/tauri-target" \
  REDUNAR_UPDATE_SOURCE_URL=https://github.com/shukzi/Redunar/releases/latest/download \
  cargo build --release --offline --manifest-path "$ui/src-tauri/Cargo.toml" \
    --bin redunar-tauri >"$fixture/native-build.log" 2>&1 || {
      tail -80 "$fixture/native-build.log" >&2
      exit 1
    }
new_binary="$fixture/tauri-target/release/redunar-tauri"
old_hash=$(rpm2cpio "$old_package" | cpio -i --to-stdout ./usr/bin/redunar-tauri 2>/dev/null | sha256sum | cut -d' ' -f1)
test -s "$new_binary"

"$snapshot/tools/stage-tauri-package.sh" "$fixture/redunar-app-package-root" >"$fixture/stage.log"
tar -C "$fixture" -czf "$fixture/rpmbuild/SOURCES/redunar-app-package-root.tar.gz" \
  redunar-app-package-root
cp "$snapshot/packaging/redunar-app.spec" "$fixture/rpmbuild/SPECS/redunar-app.spec"
rpmbuild --define "_topdir $fixture/rpmbuild" --define "_tmppath $fixture/tmp" \
  -bb "$fixture/rpmbuild/SPECS/redunar-app.spec" >"$fixture/rpm-build.log" 2>&1 || {
    tail -80 "$fixture/rpm-build.log" >&2
    exit 1
  }
new_package=$(find "$fixture/rpmbuild/RPMS" -type f -name 'redunar-app-*.rpm' -print -quit)
test -n "$new_package"
test "$(rpm -qp --qf '%{VERSION}' "$new_package")" = "$new_version"
new_hash=$(rpm2cpio "$new_package" | cpio -i --to-stdout ./usr/bin/redunar-tauri 2>/dev/null | sha256sum | cut -d' ' -f1)
if [[ $old_hash == "$new_hash" ]]; then
  printf '%s\n' 'The newer RPM has the same executable bytes as the old RPM.' >&2
  exit 1
fi
cp "$old_package" "$fixture/packages/old.rpm"
cp "$new_package" "$fixture/packages/new.rpm"

podman run -i --rm --pull=never --network=none \
  -v "$fixture/packages:/packages:ro,Z" "$image" \
  bash -s -- "$old_version" "$new_version" "$old_hash" "$new_hash" <<'CONTAINER'
set -euo pipefail
trap 'printf "Container check failed at line %s: %s\\n" "$LINENO" "$BASH_COMMAND" >&2' ERR
old=$1
new=$2
old_hash=$3
new_hash=$4
mkdir -p /home/fixture/.local/share/redunar
printf 'retain local history\n' >/home/fixture/.local/share/redunar/history.txt
rpm --nodeps --noscripts -Uvh /packages/old.rpm >/dev/null
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$old"
test "$(sha256sum /usr/bin/redunar-tauri | cut -d' ' -f1)" = "$old_hash"
# A cancelled handoff before package-manager confirmation leaves the old build.
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$old"
rpm --nodeps --noscripts -Uvh /packages/new.rpm >/dev/null
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$new"
test "$(sha256sum /usr/bin/redunar-tauri | cut -d' ' -f1)" = "$new_hash"
test "$(cat /home/fixture/.local/share/redunar/history.txt)" = 'retain local history'
rpm --nodeps --noscripts --oldpackage -Uvh /packages/old.rpm >/dev/null
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$old"
test "$(sha256sum /usr/bin/redunar-tauri | cut -d' ' -f1)" = "$old_hash"
test "$(cat /home/fixture/.local/share/redunar/history.txt)" = 'retain local history'
printf 'PASS: isolated Fedora real-binary RPM %s → %s → %s; executable and local history verified.\n' "$old" "$new" "$old"
CONTAINER

# Retain the exact pair only when a host transaction review is requested.
# The ordinary offline test still cleans its isolated build snapshot.
if [[ -n ${REDUNAR_UPDATER_REVIEW_DIR:-} ]]; then
  review_dir=$REDUNAR_UPDATER_REVIEW_DIR
  if [[ $review_dir != /* ]]; then
    review_dir="$workspace_root/$review_dir"
  fi
  mkdir -p "$review_dir"
  install -m 0644 "$fixture/packages/old.rpm" "$review_dir/redunar-app-$old_version.rpm"
  install -m 0644 "$fixture/packages/new.rpm" "$review_dir/redunar-app-$new_version.rpm"
  printf 'Retained host review RPMs: %s\n' "$review_dir"
fi
