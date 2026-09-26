# Sourced after workspace_root is set. Compatibility builds never share the
# host Cargo targets or the legacy parent-folder cache symlinks.
release_root=${REDUNAR_RELEASE_ROOT:-"$workspace_root/.redunar-build/linux/native/tauri/release"}
if [[ "$release_root" != /* ]]; then
  printf '%s\n' 'REDUNAR_RELEASE_ROOT must be absolute.' >&2
  exit 2
fi

verify_release_artifacts() {
  if [[ -z ${REDUNAR_RELEASE_ROOT:-} ]]; then
    if [[ ! -f "$workspace_root/.redunar-build/linux/runtime.sha256" ]]; then
      printf '%s\n' 'Run tools/build-linux-release.sh successfully before consuming release artifacts.' >&2
      return 1
    fi
    sha256sum --check --status "$workspace_root/.redunar-build/linux/runtime.sha256"
  fi
}

# Hold this descriptor through staging/packaging so a competing build cannot
# replace one component between verification and copying the payload.
lock_release_artifacts() {
  if [[ -z ${REDUNAR_RELEASE_ROOT:-} ]]; then
    exec 7<"$workspace_root/.redunar-build/linux/build.lock"
    flock -sn 7 || { printf '%s\n' 'A compatibility build is updating these artifacts.' >&2; return 1; }
  fi
}
