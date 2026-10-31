# Instant Replay contract

Replay is local and capability-gated. This guide owns recording, saving, audio,
playback, export, and clip persistence. [DESIGN.md](DESIGN.md) owns interface
rules; [PERFORMANCE.md](PERFORMANCE.md) owns resource budgets.

## Activation and settings

Supported launches from Redunar or the configured native Steam wrapper request
Replay automatically. There is no separate global or per-game Off selector.
Unsupported hardware or launch paths expose an unavailable or failed state and
do not claim that a buffer exists.

Recording settings are locked during the owned game session. The default is
60 FPS and Balanced quality. Variable FPS timestamps accepted game presents
within bounded resolution and encoder limits; it may drop frames under load.
Existing fixed 30 and 120 FPS profiles remain readable. Fixed 120 FPS keeps its
1080p limit. Supported save lengths are 15/30 seconds and 1/2/3/5/10/15 minutes;
the rolling buffer retains a bounded 15-minute horizon.

The source is the launched game's Vulkan or supported desktop OpenGL presentation
path. Replay does not capture the complete desktop or compositor scanout. A
prepared runtime can show or hide metrics without dismantling capture.

The output container is MKV or MP4. Changing it applies to future saves. The
replay folder, initial duration, menu dismissal, and shortcuts are saved in
profiles. Changing the folder does not move existing recordings. Saves respect
filesystem reserve and never delete unrelated recordings to make room.

## Shortcuts and menu

The default menu shortcut is Alt+Shift+Z; save bindings start empty. Existing
bindings, including cleared bindings, remain readable through migration. Menu,
save-only, and empty configurations are valid and do not restore defaults.

The in-game menu is rendered by the active Vulkan or OpenGL capture backend.
There is no desktop menu window. The helper uses bounded same-user input and
pointer handling. Missing keyboard or mouse access produces a clear unavailable
or view-only state. Device loss closes the menu and releases input. Escape,
unrelated keys, timeout, capture teardown, and shutdown release input control.

A saved clip reports completion only after the native save lifecycle commits it.
Buffer readiness, saving, unavailable, and failure are separate states. The
Moment saved notice is independent from metrics visibility.

## Capture and encoding

Vulkan Replay supports the implemented 8-bit and packed 10-bit swapchain paths
when the game uses Vulkan 1.1 or newer and the actual device exposes the
required format, queue, memory, external synchronization, and Vulkan Video
H.264 capabilities.

Supported desktop OpenGL uses bounded GBM buffers and external-memory
synchronization to feed the same hardware encoder. NVIDIA OpenGL uses a
UUID-matched Vulkan bridge and a bounded GPU copy between compatible handle
paths. OpenGL ES/EGL Replay remains unavailable.

The source device and encoder device must match by vendor and driver identity.
Multiple or unidentified GPUs remain gated. Vulkan extension discovery is
bounded and retries incomplete lists; a partial list never establishes support.
Allocation, fence, device-loss, resize, and cleanup failures quarantine or drop
unsafe frames without blocking presentation. There is no silent software-video
fallback.

Capture begins after encoder readiness. Accepted frames retain their timestamps.
A rolling save starts at a retained keyframe, so actual duration can be shorter
than the selected history window. Inspect the media result during acceptance.

Disk-spool pressure is repaired from the bounded in-memory Replay window in
timestamp order. The window is capped at 256 MiB, and transient queue pressure
does not discard the active segment. If the repair window is exhausted, or a
write, flush, rename, or retention operation fails, disk history becomes
unavailable rather than publishing a clip with an unreported gap. Live capture
continues in the bounded RAM window, recent RAM-backed saves remain available,
and a disk-history save reports that it must be retried. A save may also ask
for another attempt while deferred history is still catching up.

## Audio

Replay records the mixed default system output through PulseAudio or PipeWire
and can include other applications. It does not promise game-only audio. Pure
ALSA output capture is unsupported. Optional microphone capture is separate from
system output and voice-chat volume; test audio is temporary and is never added
to a clip.

Input discovery, route changes, device loss, cancellation, and shutdown use
bounded workers. Missing audio is reported as missing or silent data rather than
fabricated samples.

## Playback, trim, and export

Playback reads only validated Redunar clip paths. The original recording remains
untouched during trimming and export. Export supports MKV/MP4, cancellation,
bounded size limits, free-space checks, and atomic destination creation. A
failed export leaves the original clip and temporary files are cleaned only when
owned by Redunar.

History stores bounded session measurements and clip metadata. Filenames,
favorites, and the recoverable clip trash ledger preserve inode and path checks.
Readers cap file size and record count; writes use private temporary files,
atomic replacement, and directory synchronization.

## Validation

Use automated fixtures, media decode checks, and the native Replay probes in
[TESTING.md](TESTING.md). For a supported native Steam game, follow
[REAL-GAME-TESTING.md](REAL-GAME-TESTING.md). Record exact source, package,
hardware, driver, session, mode, output duration, decodability, audio result,
and cleanup. Synthetic or detected capability results do not qualify a real
recording.
