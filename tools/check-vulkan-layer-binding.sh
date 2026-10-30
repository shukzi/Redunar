#!/usr/bin/env bash
set -euo pipefail
workspace_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ $# != 1 || "$1" != /* || ! -f "$1" ]]; then
  printf '%s\n' 'usage: tools/check-vulkan-layer-binding.sh /absolute/capture-library.so' >&2
  exit 2
fi
fixture=$(mktemp -d /tmp/rdr-binding.XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
cc -std=c11 -Wall -Wextra -Werror -Wl,--export-dynamic \
  "$workspace_root/tools/fixtures/vulkan_layer_binding.c" -ldl -o "$fixture/check"
"$fixture/check" "$1"
