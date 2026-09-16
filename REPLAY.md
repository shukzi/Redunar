# Instant Replay contract

Current Tauri behavior, reviewed September 13, 2026. Recording and saving remain
local. This guide owns replay behavior; [DESIGN.md](DESIGN.md) owns presentation
and [PERFORMANCE.md](PERFORMANCE.md) owns resource budgets.

## Activation and settings

On a supported Redunar launch, replay is requested automatically. Recording
settings configure the buffer without a separate activation switch. Capability
and runtime validation still apply;
unsupported launches must not claim that a buffer exists.

The prepared Vulkan runtime also permits showing/hiding metrics during that game.
Hiding metrics does not dismantle capture or disable future visibility changes.
This does not allow attaching a runtime to an unrelated running game.

Recording frame rate, quality, and format are configured before launch and
locked until the owned game closes. The default is 60 FPS / Balanced. Supported
save lengths are 15/30 seconds and 1/2/3/5/10/15 minutes. The spool retains the
bounded 15-minute horizon; choosing a shorter manual save length selects a suffix,
not a smaller rolling horizon. Fixed 120 FPS is gated by display/game-surface
limits of 1080p or lower and actual encoder capability.

Global settings also owns replay folder, initial save duration, menu dismissal,
and shortcuts. Changing the folder parent keeps existing files in their old
location. No user-facing total storage quota exists; saves respect the filesystem
reserve and must not erase other recordings to make room.

## Shortcuts and menus

Fresh preferences assign **Shift+Tab** to the replay menu and **F8** to saving
30 seconds. Other duration bindings start empty. These are Redunar defaults,
not desktop bindings or a guarantee against another application's hotkey clash.

The menu and up to eight save bindings are optional. Save-only, menu-only, and
completely empty configurations persist without restoring defaults on restart.
Clearing every shortcut stops keyboard activation; automatic buffering and the
app's Save replay action remain available. Validate assigned chords for duplicates.

Tauri uses the same-user `redunar-hotkey-helper` with bounded evdev reads. It does
not request Polkit authorization or edit desktop-global shortcuts. The RPM's
logind `uaccess` rule supplies access where supported; unavailable permissions
must be reported honestly. Native dispatch works with the main webview hidden.

The shortcut opens a dedicated transparent Tauri window loading
`output/tauri-redunar/ui/replay-menu.html`. Its close/Escape/toggle and configured
outside-click behavior dismiss the menu, not Redunar. The app's Preview replay
menu is a separate in-app dialog. Exterior black bars, the main app shell, or
scrollbars are defects. Transparency/focus behavior needs compositor testing;
this desktop window is not the Vulkan metrics renderer.

A completed save drives the bottom-left **Moment saved** pill in the game,
including duration and Local library. It is independent of metrics visibility.
Do not show success on key press alone: completion must come from the native
save lifecycle. Buffer readiness, saving, unavailable, and failure are distinct.

## Capture and encoding

The production path is the private game-owned Vulkan export route, not KMS or
desktop screen scraping. Bounded DMA-BUF exports pass to a daemon-owned worker
for GPU conversion and hardware H.264 encoding, then local muxing and spooling.
Supported formats include the implemented 8-bit and packed 10-bit swapchain
paths; support still depends on usage flags, device, queue, and driver features.

Backpressure drops replay work rather than waiting for an encoder on the game's
presentation path. Resize/reset retires resources safely. Audio or recorder
failure must not stall the game. Recording has no software-video fallback;
FFmpeg's role in playback/export below is separate from live capture.

A save assembles available retained history, including the active spool tail,
into a private local MKV or MP4. It requires a healthy populated buffer and
preserves a 512 MiB filesystem reserve. Store markers, validated names, atomic
commit, no-overwrite behavior, and owned-file deletion prevent arbitrary access.
Save success is reported only after native completion and inventory revision.

## Audio behavior and output fallback

Audio uses PipeWire discovery, one long-lived `pw-cat` stream, bounded Opus
packets, and timestamped muxing. Missing or failed audio can leave video-only
recording; no microphone permission or arbitrary source selector is exposed.

When a game-owned stream cannot be identified, output-monitor fallback is
intentional and owner-approved. It keeps recordings audible when games or
launchers do not expose usable process ownership metadata. The fallback can
include audio from other applications; do not describe it as game-only capture.

`crates/redunar-capture-audio/src/source.rs` first tries a game-process-owned
playback node. If none is found, `discover_output_monitor_node` in `node.rs`
selects the first valid Audio/Sink entry. This does not verify that it is the
uniquely active/default output. Missing or ambiguous discovery and audio failures
still need honest runtime states. Validation should record which source was
selected and check both game-owned audio and the intended output fallback.

## Clip browsing, playback, and export

Native inventory supplies names, sizes, timestamps, and available game/duration
metadata. Read thumbnail/metadata files only through validated local paths.
Selection preserves the clip rail, search, scroll, and focus. One preparation
runs while a newer pending selection replaces older requests; stale completions
cannot replace the selected player. Expensive media work runs off the UI thread.

Playback lazily starts a loopback-only HTTP listener with random per-selection
256-bit tokens. It serves an already-opened validated file through GET/HEAD and
bounded ranges, checks Host and supplied Origin, and accepts no arbitrary path.
A selection change invalidates old streams; shutdown joins readers and cleans up.

Native preparation uses FFmpeg to make a private fast-start MP4. H.264 is copied;
optional Opus audio is converted to AAC for WebKitGTK. This also normalizes MP4
inputs. A bounded temporary copy, 30-second preparation deadline, and useful
conversion/storage errors protect the player. Temporary allocation can fall
back from a constrained default temporary filesystem to a private `/var/tmp`
directory. Source recordings are unchanged; prepared copies have private access
and are removed when replaced or on normal shutdown.

Playback waits for a decoded frame, not just metadata. The media-load deadline
is 15 seconds after preparation, with Retry on failure. The player follows the
clip aspect ratio without cropping. Eight filmstrip previews use one temporary
muted decoder; timeouts or missing previews must not disable valid playback.
See PERFORMANCE for worker/decoder bounds.

Export re-encodes the selected range for accurate boundaries, reports progress,
supports cancellation, and commits a separate clip. The original is never deleted
or archived as part of trimming. FFmpeg playback/export capabilities must be
validated separately from the live hardware encoder. Native checks resolve the
installed executables from PATH, check required encoders on demand, and diagnose
clip-specific missing codec/format support. Successful encoder checks are cached
for up to a minute; a missing capability is checked again on retry so installing
codecs does not require restarting the app. No app path installs or swaps packages.

The RPM requires the ffmpeg/ffprobe executable paths and a complete codec
capability that either RPM Fusion's full libraries or its standalone freeworld
library supplies. It also requires WebKit's GStreamer libav decoder plugin.
Existing compatible providers satisfy the requirements; missing requirements
are handled by the package manager. See [packaging](packaging/README.md).
Other distributions need their own packaging metadata; runtime checks do not
identify a distribution or require an RPM package name.

## Validation and history

Use [TESTING.md](TESTING.md) for deterministic media tests, native WebKit probes,
installed-component verification, and owner-controlled game acceptance. Fixture
encode success does not prove real-game frame pacing or cross-distribution
playback. Preserve original files during diagnostics; copies belong in isolated
state. Retired toggles and obsolete readiness behavior must not be reintroduced.
