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

The shortcut opens the Replay menu inside the captured game: the Vulkan layer
renders the panel into the game's own swapchain, so compositor stacking rules
and fullscreen focus games cannot hide it behind the game window. There is no
desktop menu window. The helper owns the pointer: while the menu is open it
grabs every mouse through the kernel so the game receives no pointer input, and
streams coalesced `MENU MOVE`/`MENU BUTTON` events over the replay control
socket; the daemon hit-tests them and answers `OK GRAB`/`OK RELEASE`. Escape,
any unrelated key (so Alt+Tab is never stranded), the helper's inactivity
timeout, and the daemon's command watchdog all release the mice and close the
menu. Some login sessions grant keyboard access without granting raw mouse
access. In that case the menu still renders in a view-only fallback, the save
shortcuts remain active, and the shortcuts panel reports that pointer control
is unavailable; a transient kernel mouse-grab failure uses the same fallback
instead of cancelling the menu. Composite keyboard interfaces are evaluated in
one bounded chord window so modifier and function-key reader scheduling cannot
drop Shift+F8. Shift+F8 or any unrelated key closes it. The menu renders even
when the metrics overlay is hidden. Its Vulkan surface and bounded control
outlines use rounded corners, and measured labels are centered within their
cells so scaling cannot push shortcut or status text across a divider. A replay menu requires a running captured
session, and without one the helper reports the rejection and the shortcuts
panel shows it. The app does not expose a desktop preview because the production
menu is rendered by the Vulkan layer inside the captured game.

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
Each committed clip name is queued beside the completed-save revision so the
game-session coordinator can attribute the clip to the recording game. The
attribution lands in the private bounded ledger
`replay-clip-games-v1.tsv` in the state directory and is labeling data only:
ledger failures never affect the save, and clips without a ledger line are
resolved later by matching the commit time encoded in the clip name against
exactly one recorded session window.

## Audio behavior

Replay records the default output monitor: the same mixed audio the user hears.
It does not depend on game-process ownership and does not request microphone
access. This deliberately includes other applications that play through the
same output, so it must be described as system-output audio rather than isolated
game audio.

`crates/redunar-capture-audio/src/source.rs` prefers the PulseAudio monitor API.
That works with a native PulseAudio server and with PipeWire's Pulse server. If
Pulse compatibility is absent, Redunar resolves the active PipeWire output with
`wpctl` and records it directly with `pw-cat`. Both paths produce fixed 48 kHz
stereo PCM, bounded 20 ms Opus packets, and timestamped muxing. The worker
rechecks the default route every two seconds, reconnects within 250 ms after a
route change or capture failure, and treats three seconds without samples as a
stalled transport that must be restarted. When both backends are available, a
failed or stalled recorder is retried through the other backend instead of
repeatedly selecting the same broken route. Older PulseAudio clients without
`pactl get-default-sink` use the long-standing `pactl info` result.

Redunar therefore requires either a working PulseAudio-compatible server with
`pactl` and `parec`, or a working PipeWire server with `pw-dump`, `wpctl`, and
`pw-cat`. PipeWire itself is not mandatory. A pure ALSA setup has no standard
monitor for already-mixed playback and is unsupported unless the user routes
output through PulseAudio or PipeWire. Missing audio still leaves video capture
running and must be reported honestly.

## Clip browsing, playback, and export

Native inventory supplies names, sizes, timestamps, and available game/duration
metadata. The game name follows the persisted attribution ledger first and the
unique-session window fallback otherwise; ambiguous or missing evidence shows
"Game not recorded" rather than a guess, and a trimmed export inherits its
source clip's attribution. Inventory requests first flush names committed by the
save worker into the attribution ledger, so the completion refresh includes the
recording game. Read thumbnail/metadata files only through validated local paths.
Selection preserves the clip rail, search, scroll, and focus. One preparation
runs while a newer pending selection replaces older requests; stale completions
cannot replace the selected player. A newer selection or navigation away cancels
the active native preparation. Expensive media work runs off the UI thread.

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
Startup also sweeps leftovers a crash or kill cannot clean itself: private
`/var/tmp/redunar-player-*` copies older than one hour and stale
`$XDG_RUNTIME_DIR/redunar/capture-*` session directories (including in-game
reply sockets) owned by this user. Audio-capture and FFmpeg children get
`PR_SET_PDEATHSIG`, so a killed Redunar cannot orphan a live recorder.

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
