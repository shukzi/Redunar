#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
image=registry.fedoraproject.org/fedora:44
package="$workspace_root/target/packages/redunar-app.rpm"
if [[ ! -f $package ]] || ! command -v podman >/dev/null || \
   ! podman image exists "$image"; then
  printf '%s\n' 'A local Redunar RPM and cached Fedora 44 Podman image are required; no image will be pulled.' >&2
  exit 2
fi

base_version=$(rpm -qp --qf '%{VERSION}' "$package")
if [[ ! $base_version =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  printf 'Unexpected RPM version: %s\n' "$base_version" >&2
  exit 1
fi
next_version="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.$((10#${BASH_REMATCH[3]} + 1))"
fixture=$(mktemp -d "$workspace_root/target/.updater-rpm-flow.XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS} "$fixture/tmp"
"$workspace_root/tools/stage-tauri-package.sh" "$fixture/redunar-app-package-root" >"$fixture/stage.log"
tar -C "$fixture" -czf "$fixture/rpmbuild/SOURCES/redunar-app-package-root.tar.gz" redunar-app-package-root
awk -v version="$next_version" '$1 == "Version:" { $2 = version } { print }' \
  "$workspace_root/packaging/redunar-app.spec" >"$fixture/rpmbuild/SPECS/redunar-app.spec"
rpmbuild --define "_topdir $fixture/rpmbuild" --define "_tmppath $fixture/tmp" \
  -bb "$fixture/rpmbuild/SPECS/redunar-app.spec" >"$fixture/build.log" 2>&1
synthetic=$(find "$fixture/rpmbuild/RPMS" -type f -name 'redunar-app-*.rpm' -print -quit)
test -n "$synthetic"
cp -- "$package" "$fixture/old.rpm"
cp -- "$synthetic" "$fixture/new.rpm"

# The newer RPM changes package metadata only. The test exercises Fedora's
# upgrade/cancel/rollback transaction behavior, not a new Redunar application
# binary or public update signature.
podman run -i --rm --pull=never --network=none \
  -v "$fixture:/fixture:ro,Z" "$image" \
  bash -s -- "$base_version" "$next_version" <<'CONTAINER'
set -euo pipefail
old=$1
new=$2
mkdir -p /home/fixture/.local/share/redunar
printf 'retain local history\n' >/home/fixture/.local/share/redunar/history.txt
rpm --nodeps --noscripts -Uvh /fixture/old.rpm >/dev/null
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$old"
# A cancelled handoff performs no transaction and leaves the prior package.
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$old"
rpm --nodeps --noscripts -Uvh /fixture/new.rpm >/dev/null
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$new"
test "$(cat /home/fixture/.local/share/redunar/history.txt)" = 'retain local history'
rpm --nodeps --noscripts --oldpackage -Uvh /fixture/old.rpm >/dev/null
test "$(rpm -q --qf '%{VERSION}' redunar-app)" = "$old"
test "$(cat /home/fixture/.local/share/redunar/history.txt)" = 'retain local history'
printf 'PASS: isolated Fedora RPM %s → %s → %s; skipped transaction and local history preserved.\n' "$old" "$new" "$old"
CONTAINER
