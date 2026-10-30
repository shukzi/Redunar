# Ultrawide Replay and Intel temperature — October 8, 2026

Branch `fix/ultrawide-replay`, base `a6ebea6`. Task:
[acceptance and boundaries](2026-10-08-ultrawide-task.md).
The final full-validation source manifest SHA-256 is
`e60440e677c389b4da6f10d426cd4e988cd0185a6f9840b7e62a450f29cbb787`.
Initial/final validation manifests matched. This evidence summary and index link
were added afterward; production code and tests did not change.

## Findings and change

The owner relayed working NVIDIA GPU detection/in-game metrics, missing CPU
temperature and a 5120×1440 Replay rejection. The screenshot identifies an RTX
3090 Ti and Intel Core i9-12900K; installed build and driver remain unknown.
This is tester-reported detection/metrics evidence, not successful encoding.

Shared source/protocol checks and independent encoder/import/stream checks
previously imposed a rectangular 3840×2160 limit. 5120×1440 has 7,372,800
pixels, less than the existing 8,294,400-pixel 4K budget. One shared shape check
now admits up to 8,192 pixels per axis under that same pixel and H.264 padded
frame budget. Both Vulkan and OpenGL use it; OpenGL's per-slot allocation byte
limit stays unchanged, including padding. Driver coded-extent, device/driver
identity, format, H.264 level, import and ownership gates remain active.
H.264 level selection also checks each macroblock axis, so extreme aspect
ratios cannot use a level that satisfies area/rate but exceeds its axis bound.

Variable mode previously applied the 120 FPS candidate marker to source-rate
admission and required an encoder ceiling of at least 120 FPS. Larger sources
already have a lower computed macroblock ceiling. Producers now admit their
bounded Variable source, retain their existing resolution-dependent pacing, and
the encoder/stream accepts the computed ceiling (72 FPS at 5120×1440).
Fixed 120 FPS retains its 1080p source gate. No downscaling or larger frame
memory budget was added. Source limits belong to [REPLAY](../../REPLAY.md#capture-and-encoding);
resource budgets belong to [PERFORMANCE](../../PERFORMANCE.md#replay-bounds).

CPU temperature discovery recognized only AMD `k10temp`/`zenpower`. The new
small Linux discovery module also selects readable Intel `coretemp` package
labels, never arbitrary motherboard/GPU/core readings. The live sampler still
reads its cached path. Missing sensors remain unavailable; no module is loaded
and no permissions or CPUFreq policy changes. The native Replay rejection now
refers to game resolution and recording frame rate, independently of window mode.

## Checks and review

- `tools/validate.sh full` passed both Cargo workspaces, private Rust/Node/Python
  suites, strict Clippy, formatting, syntax and production frontend build.
  Report: `.redunar-build/reports/20261008T205023Z-full.lVQDk3.log`.
- Focused coverage exercises ultrawide copied/exported protocol messages,
  Vulkan/OpenGL source selection, stream/DMA-BUF shape validation, short backing
  buffers, Variable request ceilings, H.264 axis levels, oversized rejection,
  Intel package selection, cached updates, sensor loss and malformed readings.
  Existing lifecycle/failed-shutdown/resize/release ownership fixtures passed.
- Isolated Xvfb/WebKitGTK production UI fixture passed 97 workspace checks.
  Report: `.redunar-build/reports/ultrawide-webkit.log`. Visually inspected
  `target/ui-review/ultrawide-resolution-rejection.png`: native-derived rejection
  copy is readable, fits the capture strip and clears on readiness recovery.
  Xvfb emitted its expected missing-DRI3 acceleration warnings.
- Release Vulkan/OpenGL sidecar builds passed with locked offline dependencies.
  Report: `.redunar-build/reports/ultrawide-sidecars.log`. The compiled Vulkan
  sidecar passed the nongraphics competing-symbol binding gate:
  `.redunar-build/reports/ultrawide-layer-binding.log`.
- The initial run exposed a constant test assertion rejected by Clippy and a
  UI fixture selector used on the wrong page. Both fixture mistakes were fixed
  before the passing final runs; no gate was weakened.
- Separate final review traced Steam/direct source preparation, shared wire
  validation, capture imports, service encoder construction, stream/settings
  matching, mux metadata, source replacement/rejection and failed cleanup.
  No persistence or wire-layout change, new dependency or unrelated source edit.

Release sidecar SHA-256 values:

| Artifact | SHA-256 |
| --- | --- |
| Vulkan | `b33e97dce04ee61832ab9abf03d2ecca5a742ef3060363e012b0029c37de0a96` |
| OpenGL | `dbc38c695f13c38c66abf372b54eeae855ce32869fca6e17ed0ae8dbef34ba2d` |

## Remaining acceptance and recovery

No installation, real game, live GPU encoding or publication ran. No new full
distribution package/release candidate was qualified. The tester must verify
the driver's exact coded extent, recording/save/decode/audio and sustained
pacing at 5120×1440 with matching new app/capture libraries. A driver rejection
after these software gates is a separate finding; GPU detection alone never
proves encoding. Temperature needs an exposed/readable coretemp package sensor.
NVIDIA recording remains unverified, as documented in
[hardware support](../../HARDWARE-SUPPORT.md#recorded-evidence-and-remaining-gates).

The separate CachyOS/KDE X11 ARC Raiders launch report remains unresolved in
private owner follow-ups. This change does not diagnose or fix that launch.
Old capture readers reject newly admitted shapes safely; no stored format or
existing recording needs migration. Revert the branch for recovery.
