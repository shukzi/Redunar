# Hardware and integration support

Redunar targets x86_64 Linux with native Steam. Recording, metrics, overlay,
codecs, queues, memory imports, sensors, and desktop session behavior are
capability-gated. Detection alone does not establish recording support for a
GPU, driver, game, or display configuration.

## Supported scope

| Area | Behavior |
| --- | --- |
| Desktop | Tauri with GTK3/WebKitGTK on Wayland or X11. Validate the two sessions separately. |
| CPU and memory | Cached Linux utilization, supported temperature sensors, RAM, and available GPU memory. Missing or unreadable values remain unavailable. |
| GPU metrics | AMD `amdgpu`/DRM metrics and NVIDIA metrics through the installed NVML library. Device identity is matched by physical render GPU and PCI topology; ambiguous topology remains gated. |
| Vulkan games | Frame metrics, overlay, in-game Replay menu, and hardware H.264 Replay through Vulkan Video when the game device, format, queue, memory, and encoder capabilities pass. |
| Desktop OpenGL games | Launch-scoped GLX/EGL/SDL metrics. Supported desktop OpenGL paths can export eligible frames to the existing Vulkan Video encoder when external-memory and device identity checks pass. OpenGL ES/EGL Replay remains unavailable. |
| Native Steam | Imported native games can use the Redunar Steam wrapper and session bridge. The Linux user service manager is required when the wrapper starts a closed Redunar instance. |
| Flatpak Steam | Discovery may work, but host-layer capture and Replay are outside the supported sandbox boundary. |
| Replay | Fixed 60 FPS and Variable FPS modes, bounded 15-minute rolling history, MKV/MP4 output, local playback/export, mixed system-output audio, and optional microphone capture. |
| Packages | Debian, Fedora, openSUSE, native Arch, and portable x86_64 packages with signed release metadata. ARM and immutable-system packages are not qualified. |

NVIDIA recording uses Vulkan Video H.264. The Vulkan and OpenGL producers attach
the current game's device and driver identity to each source. The encoder must
match those identities before importing frames; Redunar never silently selects
another GPU. The qualified NVIDIA evidence covers the RTX 2060 SUPER path and
its tested Fedora driver/build. It does not guarantee every NVIDIA GPU or
hybrid topology.

The OpenGL bridge uses bounded GPU buffers and synchronization. It does not
copy captured pixels through a software-video fallback. Missing external-memory
extensions, unsupported formats, queue limits, device loss, stale sensors, and
allocation failures produce unavailable or failed states.

## Launch and session limits

The native Steam wrapper verifies the same-user process and local game identity.
If its user service manager is unavailable, launching Redunar first can avoid
the startup dependency while leaving the game launch usable. A running game is
never attached after the supported launch path has been missed.

Replay captures the game's Vulkan or supported desktop OpenGL presentation, not
the complete desktop or compositor scanout. Variable FPS has resolution-based
admission limits and may drop frames under load. No claim covers every
fullscreen, compositor, or game configuration.

Audio records the mixed default system output and may include other programs.
Pure ALSA output capture and game-only audio isolation are unsupported claims.

## Failure behavior

- Missing sensors affect only their own values.
- Device loss clears stale readings and blocks unsafe frame reuse.
- Multiple or unidentified render GPUs remain unavailable until identity is
  unambiguous.
- Unsupported driver, codec, queue, format, memory, or session capabilities
  fail closed without software video.
- Missing tray or input permissions must leave a usable fallback and explain
  the unavailable feature.

Use [REPLAY.md](REPLAY.md) for recording behavior, [PERFORMANCE.md](PERFORMANCE.md)
for budgets, and [REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for hardware runs.
