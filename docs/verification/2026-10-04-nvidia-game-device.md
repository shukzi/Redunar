# NVIDIA beta game device attribution — October 4, 2026

## Build and scope

Local implementation on `fix/nvidia-game-device`, based on
`f6cd5676de77e367f43f769fe03b6cd88782a1ff`, package version `0.1.13`.
The full validation and compatibility build used source-manifest SHA-256
`06a445c2d8cb13b90aeb148c8fc9c3cbedef62c8034635849f4e6db6fe6e7028`.
This evidence summary was added afterward; implementation files are unchanged.
No installation, live game, NVIDIA hardware test, external publication, or
version bump was performed.

The reported RTX 3090 Ti plus Intel i915 topology was correctly enumerated.
The previous single-render-GPU gate withheld NVIDIA before attempting its
capabilities. Card/render-node numbering cannot determine which GPU a game uses.

## Implemented behavior

- Beta discovery admits one identifiable NVIDIA physical render GPU alongside
  known Intel/AMD render GPUs. Alias nodes are deduplicated; unknown topology
  and multiple NVIDIA render GPUs stay gated. NVML retains its PCI selection.
- Vulkan reports the actual presenting logical device's physical device UUIDs.
  Desktop OpenGL queries its current context's external-memory UUIDs and binds
  GBM to the sole physical render GPU for that vendor. GPU 0/card0/renderD128
  have no special authority.
- Private protocol version 7 carries bounded vendor/device/driver identity in
  GPU reports and Replay source/copy/export metadata. The daemon requires the
  encoder to match the game's UUIDs before import, and rejects changed/missing
  identity on subsequent frames. Beta preference never overrides an AMD game
  with an installed NVIDIA encoder.
- Overlay hardware and native history use the capture vendor only when its
  physical GPU is unique. Missing identity omits GPU readings while preserving
  CPU/frame measurements. Completion, failure, and new producer startup clear
  the attribution.
- Debug logs report source identity availability, selected vendor, successful
  matching, and allowlisted capability failures. UUIDs/PCI addresses are not
  printed. OpenGL identity failure has a plain Replay-unavailable message.

## Verification

Host: x86_64 CachyOS, Linux `7.2.9-1-cachyos`; ordinary tests used fake hardware,
private fixture state, and checkout-owned targets.

- `tools/validate.sh full`: passed both Cargo workspaces, Rust/Node tests,
  frontend production build, formatting, syntax, and strict Clippy.
  Report: `.redunar-build/reports/20261004T213920Z-full.GxIYiW.log`.
  A final repeat is retained in
  `.redunar-build/reports/nvidia-game-device-final-full.log`.
- Regressions cover both card/render-node orders, hybrid/duplicate/unknown
  topology, protocol round trips and malformed identity, GL extension and
  multi-device rejection, Vulkan UUID/driver mismatch, private socket/FD
  identity handoff and ACK cleanup, missing identity before native startup,
  GPU changes before import, stale lifecycle state, and game telemetry selection.
- Isolated WebKitGTK workspace fixture: 83 checks passed. Replay fixture passed
  its existing media, layout, trimming, and unavailable-state checks.
  History and Replay screenshots were inspected. The new GPU-identity message
  was additionally injected into the built UI fixture at normal/compact widths;
  no production UI source or user state was changed for that check.
- `tools/check-tauri-release.sh`: passed the offline Debian 12 compatibility
  build, license checks, matching runtime/sidecar staging, metadata validation,
  DEB/portable/Arch/openSUSE/Fedora package checks, and local test signatures.
  All six runtime artifacts require at most glibc 2.34, within the 2.36 baseline.
  Host RPM tools emitted database-lock warnings about the absent host RPM
  database; package metadata assertions and the gate still passed. No host
  package transaction was run.
- Changed documentation links and `git diff --check`: passed. Existing historical
  links to external private build artifacts were preserved.

Compatibility artifact SHA-256:

| Artifact | SHA-256 |
| --- | --- |
| `redunar-tauri` | `9fa17fd103eded190e9e4c3ebb0eb588f9ece4cf37a7cbe18a5112f4c089b426` |
| `libredunar_capture_vulkan.so` | `4c8663ae1c2be21ba5352bb1816bae9a5c0853ae187f224466c2a9ac98a1e045` |
| `libredunar_capture_opengl.so` | `1c65bdceb6444f7a115507f0fa31bcdc996f07123561444391e91c9e5a133b0e` |

Full artifact/source manifests and build output remain in the checkout-owned
`.redunar-build/linux/` and `.redunar-build/reports/` directories.

## Separate review and remaining limits

Reviewed production native/service/producers/encoder callers, effective profile
inheritance, FD ownership and release routes, shutdown/failure clearing, unsupported
hardware, bounded caches, and diagnostic privacy after implementation. Existing
spool/history/profile formats, recordings, and release ownership are unchanged.
The retained KMS diagnostic keeps its separate entry point without authorizing
production game capture that lacks identity.

Both capture sidecars and the app must come from the same protocol-7 build, and
a fresh game process must load them. Older protocol messages are rejected;
there is no persisted-format migration. Multiple same-vendor devices remain
unavailable for telemetry/GL allocator attribution. The GL allocator belongs to
one GPU for the process lifetime, so changing GPU requires relaunching the game.

NVIDIA driver extensions, GL/Vulkan UUID compatibility, DMA-BUF import, real
sensor values, playable recording, and sustained frame pacing remain unverified
on NVIDIA. The separately authorized tester run must exercise both supported
graphics APIs and report the exact package/sidecar hashes and driver. Follow
[the hardware test procedure](../../TESTING.md#nvidia-beta-first-hardware-test);
capability rejections are findings, not reasons to bypass a gate.
