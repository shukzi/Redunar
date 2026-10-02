#!/usr/bin/env bash
# Local and CI entry point. Hardware, installation and publication are separate.
set -euo pipefail
workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$workspace_root"
mode=${1:-full}
case "$mode" in quick|full|release) ;; *) printf '%s\n' 'usage: tools/validate.sh [quick|full|release]' >&2; exit 2 ;; esac
build_root="$workspace_root/.redunar-build"
mkdir -p "$build_root/reports" "$build_root/checks"
exec 9>"$build_root/validation.lock"
flock -n 9 || { printf '%s\n' 'Validation is already running in this checkout.' >&2; exit 1; }
report=$(mktemp "$build_root/reports/$(date -u +%Y%m%dT%H%M%SZ)-$mode.XXXXXX.log")
exec > >(tee "$report") 2>&1
trap 'result=$?; printf "result=%s\nreport=%s\n" "$result" "$report"' EXIT
printf 'date=%s\ncommit=%s\nmode=%s\n' "$(date -u +%FT%TZ)" "$(git rev-parse HEAD)" "$mode"
git status --short
python3 tools/source-snapshot.py --manifest-only "$workspace_root" "$report.source.json"
rustc --version
node --version
git diff --check
python3 -m unittest discover -s tools/tests -p 'test_*.py'
for script in tools/*.sh tools/lib/*.sh; do bash -n "$script"; done
# Check the public POSIX entry point before any expensive native compilation.
sh -n install.sh
if command -v dash >/dev/null 2>&1; then dash -n install.sh; fi
cargo fmt --all -- --check
cargo fmt --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all -- --check
root_target="$build_root/checks/root"
tauri_target="$build_root/checks/tauri"
cargo check --locked --offline --workspace --all-targets --target-dir "$root_target"
# Tauri embeds ui/dist even for checks; always build it before checking a fresh checkout.
npm --prefix output/tauri-redunar run build
cargo check --locked --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets --target-dir "$tauri_target"
node --test output/tauri-redunar/tests/*.test.mjs
if [[ "$mode" != quick ]]; then
  # Keep Unix socket fixture paths short. Tests create and clean private children.
  TMPDIR=/tmp cargo test --locked --offline --workspace --target-dir "$root_target"
  TMPDIR=/tmp cargo test --locked --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets --target-dir "$tauri_target"
  cargo clippy --locked --offline --workspace --all-targets --target-dir "$root_target" -- -D warnings
  cargo clippy --locked --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets --target-dir "$tauri_target" -- -D warnings
fi
if [[ "$mode" == release ]]; then tools/check-tauri-release.sh; fi
python3 tools/source-snapshot.py --manifest-only "$workspace_root" "$report.final-source.json"
if ! cmp -s "$report.source.json" "$report.final-source.json"; then
  printf '%s\n' 'Source changed during validation; re-run on the final tree.' >&2
  exit 1
fi
printf '%s\n' 'Requested automated checks passed. Hardware and installed-runtime acceptance are separate.'
