# Redunar roadmap

Redunar is a Linux gaming application for Instant Replay, frame and system
metrics, and an in-game overlay. Current source and releases target x86_64
Linux with native Steam. `HARDWARE-SUPPORT.md` defines the tested graphics and
encoder scope.

## Current priorities

1. Qualify the next release from its exact source commit, including automated,
   packaging, installed-runtime, and affected real-game checks.
2. Improve Steam activation, capture readiness, audio, playback/export, and
   session cleanup from reproducible reports.
3. Recheck Wayland and X11 behavior and distribution-specific dependencies
   separately.
4. Measure real-use performance with the budgets in `PERFORMANCE.md` without
   promising an FPS gain.
5. Keep signed update checks, package transitions, and saved-data compatibility
   covered by focused tests.

## Product scope

The current product includes Overview, Library, Global settings, Instant Replay,
History, Settings, local game profiles, native Steam launch, frame metrics,
system metrics, capability-gated Vulkan and supported desktop OpenGL paths,
local clip playback/export, and signed package updates.

NVIDIA support uses hardware capability checks and the qualified Vulkan Video
path. AMD/RADV behavior remains a supported regression target. Missing driver,
codec, sensor, queue, memory-import, or overlay capabilities must remain
visible as unavailable or failed states.

Flatpak Steam capture, ARM builds, OpenGL ES Replay, late attachment, production
KMS capture, cloud metadata, and accounts remain outside the current scope.
Desktop Replay is future work and must be designed and measured separately from
game capture.

## Development rules

Use focused branches and the validation entry points in `CONTRIBUTING.md` and
`TESTING.md`. Preserve package identity, signing keys, saved-data readers,
recordings, and required notices. New support requires implementation,
capability checks, regression coverage, and evidence for the exact hardware and
build involved.
