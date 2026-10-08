# Hardware and integration support

Reviewed October 5, 2026 against the local implementation and retained test
records. The known tested baseline is **x86_64 Linux with AMD hardware**. This
is narrower than a future cross-vendor goal; other setups may be tried and
reported after publication. A detected interface or a successful fixture does
not establish support across every game or driver.

## Implemented scope

| Area | Current behavior and limits |
| --- | --- |
| Desktop | Tauri using GTK3/WebKitGTK on Linux; Wayland/X11 behavior needs separate validation. |
| CPU | Cached Linux identity/utilization, AMD `k10temp`/`zenpower` temperature and Intel `coretemp` package temperature where exposed. Sensor paths are discovered once; missing/unreadable package sensors stay unavailable. CPUFreq adapters remain AMD-oriented; do not apply AMD-specific interpretation to other vendors. |
| GPU | AMD `amdgpu`/DRM metrics where exposed: utilization, temperature, clock, VRAM, power. Beta access admits one identifiable NVIDIA render GPU using the installed driver's NVML library, keyed by PCI address, including alongside known Intel/AMD render GPUs. Game measurements select the capture vendor only when its physical GPU is unique. Missing identity/sensors stay unavailable. NVIDIA detection and in-game metrics have the October 8 tester report below; NVIDIA encoding remains unverified. |
| Memory | Linux RAM and available GPU-memory readings feed System metrics. |
| Catalog | Local IDs, direct launch records, native/Flatpak Steam discovery, and supported direct XDG Game entries. Import reviews installed entries, not arbitrary running helpers. |
| Native Steam launch | The app-specific wrapper prepares an imported game when Play is clicked in native Steam. Starting a closed Redunar requires a working Linux user service manager; the app must run outside Steam's game reaper. Without this capability the original game still launches; opening Redunar first avoids the startup dependency. Live requests verify the same-user wrapper and unique local identity; app-initiated launches verify saved launch options. The owner confirmed ARC Raiders save/playback with video and audio on October 6 at `163e75a`, then observed a stuck exit. Fresh-game acceptance of the corrected bootstrap remains pending. |
| Flatpak Steam | Discovery does not imply capture works inside its sandbox. Host-layer forwarding and capture remain unsupported; Flatpak is outside the supported scope by owner decision on 2026-09-24. |
| Direct Vulkan game | Private launch-time layer provides telemetry, metrics HUD, and eligible replay capture. Runtime is prepared while metrics are hidden so visibility can change later. |
| Direct or native-Steam OpenGL game | Launch-scoped GLX/EGL interposition provides frame telemetry and bounded Compact, FPS only, Detailed, and Custom metrics for owned direct launches and configured native Steam wrapper launches. Guarded SDL dynamic-API interception covers managed-runtime deep binding such as .NET P/Invoke without a startup timing window. On the validated AMD/RADV host, desktop GLX and SDL OpenGL can export fixed-pool GBM buffers into the existing Vulkan Video H.264 Replay path at 30 or 60 FPS; the existing capability gate also passed the bounded 1080p/120 probe. The in-game Replay menu and completed-save notice render independently from metrics. EGL/OpenGL ES remains metrics-only, and Flatpak capture is out of scope. |
| Replay video | Hardware Vulkan Video H.264 path on supported AMD/RADV; bounded GPU export/conversion, local spool, MKV/MP4 saves. Fixed 60 FPS and Variable FPS recording modes use the launched game's eligible presents; Variable caps admission at 240 FPS at 1080p or 144 FPS at 2560×1440, with dropped frames possible under load. Beta access admits one identifiable NVIDIA render GPU, including with Intel/AMD, through the same Vulkan Video capability gates. Both Vulkan and supported desktop OpenGL exports must match the game's device/driver UUIDs before encoding. Successful NVIDIA recording is unverified. No live software-video fallback. |
| Swapchains | Implemented 8-bit RGBA/BGRA and packed 10-bit conversion paths; actual usage flags, queues, format, resolution, and encoder limits govern acceptance. |
| Overlay | Compact, FPS only, Detailed, Custom; four corners, bounded scale/opacity, live visibility and independent saved feedback. |
| Replay menu | The capture backend renders the in-game menu into the game presentation. The app's overlay-appearance preview is separate; pointer grab remains subject to local input permissions. |
| Audio | Default-output monitor through PulseAudio or PipeWire plus Opus; the mixed output can include other applications. Pure ALSA output capture is not supported. See REPLAY. |
| Shortcuts/tray | Same-user evdev helper and native StatusNotifierItem/DBusMenu tray client. A desktop tray host is required; no AppIndicator library is required. Missing input access or tray host must produce a usable fallback. Empty shortcuts and tray-disabled startup are supported. |
| Packaging | Debian 12/glibc 2.36 baseline binaries, Fedora/openSUSE RPMs, a DEB, native Arch package, portable payload, signed checksums, and a distro-detecting installer template build locally. Installed-runtime evidence remains Fedora 44 only; no published release, verified second distribution, immutable-system package, or ARM build is implied. |

The monitor caches discovery, samples hardware at 1 Hz, and performs bounded
same-user process scans at the slower cadence in [PERFORMANCE.md](PERFORMANCE.md).
Runtime process supervision is still needed even though Library no longer imports
running-process candidates. Ordinary tests use fake `/proc` and `/sys` data.

NVIDIA beta topology deduplicates physical render devices, excluding display-only
DRM cards. One NVIDIA render GPU alongside identified Intel/AMD render GPUs is
eligible without assuming that `card0`, `renderD128`, or GPU index 0 is the game
GPU. Unknown topology and multiple NVIDIA render GPUs remain gated. OpenGL GBM
allocation selects the sole physical device for its current context's vendor;
multiple devices of that vendor remain unavailable. Its allocator is bound to
one game GPU for the process lifetime; changing GPU requires a fresh process.
The encoder independently verifies the source device and driver UUIDs. Neither
UUID nor PCI address is emitted in normal diagnostic logs.
NVML readiness, sensor availability, device loss, and cleanup are distinct
diagnostic states. Device loss clears old sensor values; recovery reuses the
same cached device identity at 5, 30, and 120 seconds, with three lifetime
attempts. Missing optional sensors do not become zero readings. These behaviors
have fake-library coverage, not NVIDIA hardware evidence. See the
[first hardware test](TESTING.md#nvidia-beta-first-hardware-test).

## Recorded evidence and remaining gates

On October 8, the owner relayed an RTX 3090 Ti / Intel Core i9-12900K tester's
working GPU detection and in-game metrics, absent CPU temperature and Replay
rejection at 5120×1440. The installed build/driver were not provided, and no
recording succeeded in that report. The follow-up admits the ultrawide shape
within the existing pixel budget and discovers Intel package temperature; see
[Replay source limits](REPLAY.md#capture-and-encoding). This report does not
qualify NVIDIA encoding or identify the separate KDE/X11 launch failure.

The retained AMD baseline was recorded on Fedora 44, Ryzen 7 5800X, Radeon RX 6800 XT/Navi 21,
using RADV with Vulkan Video support exposed by its installed Mesa build.
Historical controlled probes cover native Wayland/XCB presentation, packed
10-bit conversion, 30/60 FPS recording, resize/reset, and 4K output. They do not
establish universal throughput or performance at those modes.

The [October 5 combined-build check](docs/verification/2026-10-05-combined-live-check.md)
records finite Vulkan and desktop OpenGL Replay saves/decodes on CachyOS with an
RX 6800 XT/RADV. It includes the Linux Vulkan symbol-binding crash fix; it adds
no NVIDIA or real-game qualification.

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

GLX providers loaded with private ELF visibility, or handle-specific targets
that differ from the interposer's forwarding provider, retain their original
swap and context-destruction calls. Redunar bypasses those lookups, so frame
metrics/Replay from that path are unavailable. This compatibility guard is not
evidence that the reported CachyOS/KDE X11 ARC Raiders launch failure is fixed.

- OpenGL ES/EGL Replay capture, late injection into already-running games, and
  broad real-game OpenGL Replay qualification beyond the owner acceptance set.
- Flatpak capture bridging or broad non-Steam launcher integration.
- Verified NVIDIA encoding/monitoring parity or hybrid hardware qualification, ARM/aarch64,
  or a cross-driver guarantee.
- Firmware, voltage, or automatic hardware-policy changes.
- Game-only audio isolation; Replay records the active mixed system output.
- A claim that all fullscreen/compositor configurations support the replay menu.

Missing optional integrations must not break unrelated read-only monitoring.
Keep capability checks explicit and vendor handling isolated. Do not relabel an
unsupported path as supported merely to match a broader product aspiration.
