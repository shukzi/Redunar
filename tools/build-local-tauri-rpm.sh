#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_directory=${1:-"$workspace_root/target/packages"}

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace_root/$output_directory"
fi
if ! command -v rpmbuild >/dev/null 2>&1; then
  printf '%s\n' 'rpmbuild is required (Fedora package: rpm-build)' >&2
  exit 1
fi

mkdir -p "$workspace_root/target"
build_root=$(mktemp -d "$workspace_root/target/.tauri-rpm-build.XXXXXX")
trap 'rm -rf -- "$build_root"' EXIT
mkdir -p "$build_root/tmp" "$build_root/rpmbuild"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
export TMPDIR="$build_root/tmp"

package_root="$build_root/redunar-app-package-root"
"$workspace_root/tools/stage-tauri-package.sh" "$package_root"
tar -C "$build_root" -czf \
  "$build_root/rpmbuild/SOURCES/redunar-app-package-root.tar.gz" \
  redunar-app-package-root
install -m 0644 "$workspace_root/packaging/redunar-app.spec" \
  "$build_root/rpmbuild/SPECS/redunar-app.spec"

rpmbuild --define "_topdir $build_root/rpmbuild" \
  --define "_tmppath $build_root/tmp" \
  -bb "$build_root/rpmbuild/SPECS/redunar-app.spec"

rpm_path=$(find "$build_root/rpmbuild/RPMS" -type f -name 'redunar-app-*.rpm' -print -quit)
if [[ -z "$rpm_path" ]]; then
  printf '%s\n' 'rpmbuild did not produce a Tauri Redunar RPM' >&2
  exit 1
fi
if [[ "$(rpm -qp --qf '%{NAME}' "$rpm_path")" != 'redunar-app' ]]; then
  printf '%s\n' 'local Tauri Redunar RPM must use the redunar-app package name' >&2
  exit 1
fi

rpm_scripts=$(rpm -qp --scripts "$rpm_path")
if ! grep -Fq 'postinstall scriptlet' <<<"$rpm_scripts" || \
   ! grep -Fq '/usr/bin/udevadm control --reload-rules' <<<"$rpm_scripts" || \
   ! grep -Fq '/usr/bin/udevadm trigger --subsystem-match=input --sysname-match=event* --property-match=ID_INPUT_KEYBOARD=1 --action=change' <<<"$rpm_scripts" || \
   ! grep -Fq '/usr/bin/udevadm trigger --subsystem-match=input --sysname-match=event* --property-match=ID_INPUT_MOUSE=1 --action=change' <<<"$rpm_scripts" || \
   grep -Eiq 'pkexec|polkit|steam|/usr/bin/sudo' <<<"$rpm_scripts"; then
  printf '%s\n' 'local Tauri Redunar RPM must contain only the fixed udev refresh post-install script' >&2
  exit 1
fi
if rpm -qp --filecaps "$rpm_path" | grep -Eq '[[:space:]]cap_[^[:space:]]+'; then
  printf '%s\n' 'local Tauri Redunar RPM must not contain file capabilities' >&2
  exit 1
fi
helper_record=$(rpm -qp --qf '[%{FILENAMES} %{FILEMODES:perms}\n]' "$rpm_path" \
  | awk '$1 == "/usr/libexec/redunar-hotkey-helper" { print $1, $2 }')
if [[ "$helper_record" != "/usr/libexec/redunar-hotkey-helper -rwxr-xr-x" ]]; then
  printf 'unexpected Tauri shortcut helper mode in RPM: %s\n' "$helper_record" >&2
  exit 1
fi
if rpm -qlp "$rpm_path" | grep -Eq '/redunar-kms-helper$|/gg\.redunar\.replay-capture\.policy$'; then
  printf '%s\n' 'local Tauri Redunar RPM must not contain a privileged Replay capture path' >&2
  exit 1
fi
if ! rpm -qlp "$rpm_path" | grep -Fxq '/usr/lib/udev/rules.d/70-redunar-hotkeys.rules'; then
  printf '%s\n' 'local Tauri Redunar RPM must install the input uaccess rule' >&2
  exit 1
fi

mkdir -p "$output_directory"
# Keep a single handoff artifact for local testing. Each build replaces the
# previous RPM so the documented reinstall command always has one clear file.
find "$output_directory" -maxdepth 1 -type f \( -name 'redunar-app.rpm' -o -name 'redunar-app-*.rpm' \) -delete
final_path="$output_directory/redunar-app.rpm"
install -m 0644 "$rpm_path" "$final_path"
printf 'Built local Tauri Redunar RPM: %s\n' "$final_path"
