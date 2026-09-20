# Hardware and integration support

Reviewed September 20, 2026 against the local implementation and retained test
records. The known tested baseline is **x86_64 Linux with AMD hardware**. This
is narrower than a future cross-vendor goal; other setups may be tried and
reported after publication. A detected interface or a successful fixture does
not establish support across every game or driver.

## Implemented scope

| Area | Current behavior and limits |
| --- | --- |
| Desktop | Tauri using GTK3/WebKitGTK on Linux; Wayland/X11 behavior needs separate validation. |
| CPU | Cached Linux identity/utilization and AMD-oriented sensor/CPUFreq adapters. Do not apply AMD-specific interpretation to other vendors. |
| GPU | AMD `amdgpu`/DRM metrics where exposed: utilization, temperature, clock, VRAM, power. Missing readings remain unavailable; NVIDIA parity is not implemented or verified. |
| Memory | Linux RAM and available GPU-memory readings feed System metrics. |
| Catalog | Local IDs, direct launch records, native/Flatpak Steam discovery, and supported direct XDG Game entries. Import reviews installed entries, not arbitrary running helpers. |
| Native Steam launch | Private wrapper/broker and saved launch options are implemented. Verify the actual game's configuration; wrapper existence alone is insufficient. |
| Flatpak Steam | Discovery does not imply capture works inside its sandbox. Host-layer forwarding/capture remains unsupported without a validated bridge. |
| Direct Vulkan game | Private launch-time layer provides telemetry, metrics HUD, and eligible replay capture. Runtime is prepared while metrics are hidden so visibility can change later. |
| Replay video | Hardware Vulkan Video H.264 path on supported AMD/RADV; bounded GPU export/conversion, local spool, MKV/MP4 saves. No live software-video fallback. |
| Swapchains | Implemented 8-bit RGBA/BGRA and packed 10-bit conversion paths; actual usage flags, queues, format, resolution, and encoder limits govern acceptance. |
| Overlay | Compact, FPS only, Detailed, Custom; four corners, bounded scale/opacity, live visibility and independent saved feedback. |
| Replay menu | Dedicated transparent Tauri desktop window; focus, layering and transparency remain compositor-dependent. App preview is a separate dialog. |
| Audio | Default-output monitor through PulseAudio or PipeWire plus Opus; the mixed output can include other applications. Pure ALSA output capture is not supported. See REPLAY. |
| Shortcuts/tray | Same-user evdev helper and optional tray provider. No permission/provider must produce a usable, honest fallback. Empty shortcuts and tray-disabled startup are supported. |
| Packaging | Debian 12/glibc 2.36 baseline binaries, Fedora/openSUSE RPMs, a DEB, native Arch package, portable payload, signed checksums, and a distro-detecting installer template build locally. Installed-runtime evidence remains Fedora 44 only; no published release, verified second distribution, immutable-system package, or ARM build is implied. |

The monitor caches discovery, samples hardware at 1 Hz, and performs bounded
same-user process scans at the slower cadence in [PERFORMANCE.md](PERFORMANCE.md).
Runtime process supervision is still needed even though Library no longer imports
running-process candidates. Ordinary tests use fake `/proc` and `/sys` data.

## Recorded evidence and remaining gates

The development host is Fedora 44, Ryzen 7 5800X, Radeon RX 6800 XT/Navi 21,
using RADV with Vulkan Video support exposed by its installed Mesa build.
Historical controlled probes cover native Wayland/XCB presentation, packed
10-bit conversion, 30/60 FPS recording, resize/reset, and 4K output. They do not
establish universal throughput or performance at those modes.

The retained KMS/DRM examples are explicit engineering diagnostics outside the
production per-game Replay path. Their ability to enumerate an output, open a
device, or initialize an encoder is not a desktop-capture support claim. Their
commands, effects, and evidence boundaries are documented in
[TESTING.md](TESTING.md#retained-engineering-diagnostics).

Retained reports record owner runs with ARC Raiders and PEAK, synthetic native
session/replay acceptance, and local/Flatpak-runtime codec checks. Counter-Strike
2 and another host's distribution/WebKit integration are deferred from the
initial release. Treat each report as evidence for its named build and host,
not certification of later changes. [Real-game testing](REAL-GAME-TESTING.md)
and [ROADMAP.md](ROADMAP.md) own the remaining work.

## Outside current support

- OpenGL capture and late injection into already-running games.
- A validated Flatpak capture bridge or broad non-Steam launcher integration.
- NVIDIA encoding/monitoring parity, ARM/aarch64, or a cross-driver guarantee.
- Firmware, voltage, or automatic hardware-policy changes.
- Game-only audio isolation; Replay records the active mixed system output.
- A claim that all fullscreen/compositor configurations support the replay menu.

Missing optional integrations must not break unrelated read-only monitoring.
Keep capability checks explicit and vendor handling isolated. Do not relabel an
unsupported path as supported merely to match a broader product aspiration.
