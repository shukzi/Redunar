# NVIDIA and desktop integration — October 29, 2026

The existing Tauri app now admits eligible NVIDIA hardware without a separate
Stable/Beta selection. Legacy saved preferences remain readable. Vulkan Video
continues to own hardware encoding; unsupported capabilities remain unavailable.

## Source and capture behavior

The source baseline is `b89eb7f5d533c3b04640400207e21e0f8722a1da` (v0.1.30).
NVIDIA Vulkan export prefers unmapped device-local memory to a host-visible BAR
heap. Video-session state may use a driver-permitted non-local memory type;
frame pixels retain their GPU-only path. Recording skips pre-initialization
NVIDIA backlog without retiming frames or changing AMD startup behavior.

Desktop OpenGL uses UUID-matched Vulkan allocations with separate opaque-FD and
DMA-BUF backing. External binary semaphores and API resource barriers establish
GL/Vulkan visibility. Failed submissions are detached from chronological
publication while unproven GPU storage remains retained. Transient allocation
failures use bounded retry/backoff. Native mock regressions exercise real API
error results; separate driver injections exercise recovery after submission.

A bounded NVIDIA-only linear-buffer import cache retains completed imports by
allocation and layout identity with an owned descriptor pin. In-flight imports
cannot be reused. AMD and DRM-image import policies retain their prior behavior.
UI polling retains unchanged text/control state; real changes still update.
Read-only hardware discovery runs off the window thread. Native wheel handling,
fonts, appearance, profile ownership and persistence formats are unchanged.

## Recorded hardware acceptance

Fedora 44 KDE Wayland, RTX 2060 SUPER, driver 615.78.08: bounded desktop OpenGL
short saves, submit/fence/allocation recovery, resize, context recreation and
generated 3840×2160 output passed. Vulkan XCB and native Wayland mailbox saves
passed complete decode, timestamps, audio and teardown checks at 1080p and
generated 4K. Generated 4K is not a physical 4K display/gameplay observation.
The recorded PEAK run saved and decoded 2560×1440 Variable FPS video with
non-silent mixed audio and normal teardown. Capture-library hashes matched the
package used for those checks. Personal recordings are not release inputs.

The installed Fedora desktop follow-up package has SHA-256
`77bc760f3aceb3315471fa018c5852b1c17deda80019cc93c4ba1c5c917bd85b`.
Its capture libraries/helpers are byte-identical to the preceding recording-tested
package; its app executable adds the desktop update-retention/discovery changes.
The maintainer reported unchanged scrolling feel and retained those changes.

The primary CachyOS AMD test package has SHA-256
`534784510f8c58d74bf8ff8a7635b57633a6a6523f56ee26e5be311d4cb00b36`.
All 29 installed file hashes/modes and fresh native app/helper executable hashes
matched that exact Arch package. The helper had readable keyboard inputs. Saved
preferences, game catalog and history metadata were unchanged after restart.
The maintainer confirmed desktop use, game launch, overlay, Replay save/playback
with audio and normal game exit. This is maintainer-reported functional
acceptance, separate from automated media inspection on the NVIDIA host.

Both tested packages retain the v0.1.30 version label with the new source changes;
they are not the published v0.1.30 artifacts. The release metadata is advanced to
v0.1.31. Final release qualification records the exact integrated source and
versioned package hashes; these test hashes do not identify those future assets.

## Automated verification

Canonical full validation on the returned source passed 885 Rust tests across
both Cargo graphs and their example targets, with 12 manual tests ignored.
All 158 frontend tests, formatting, tooling fixtures and strict Clippy passed.
The production frontend and native release build passed. WebKit checks passed:
workspace 152, refinements 22, audio controls 21, performance 11, native wheel 10
and three retained-text groups. Settings and microphone layouts were inspected.
The competing-Vulkan-symbol binding check passed. Six native components matched
the native Arch package payload. These checks do not measure game FPS gains.

## Compatibility and recovery

No persisted-format migration is introduced. Keep legacy preference readers and
the existing hardware admission checks. Preserve known-good packages and saved
settings for recovery; recordings are not removed. Restart the app after updating
and relaunch games so their private capture components match the installed app.
