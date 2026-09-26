# Instant Replay contract

Current Tauri behavior, reviewed September 26, 2026. Recording and saving remain
local. This guide owns replay behavior; [DESIGN.md](DESIGN.md) owns presentation
and [PERFORMANCE.md](PERFORMANCE.md) owns resource budgets.

## Activation and settings

On a supported Redunar launch, replay is requested automatically. Recording
settings configure the buffer without a separate activation switch. Capability
and runtime validation still apply;
unsupported launches must not claim that a buffer exists.

The prepared Vulkan or supported desktop-OpenGL runtime also permits
showing/hiding metrics during that game.
Hiding metrics does not dismantle capture or disable future visibility changes.
This does not allow attaching a runtime to an unrelated running game.

Recording mode and quality are configured before launch and locked until the
owned game closes. **60 FPS** keeps fixed-cadence capture; **Variable FPS**
timestamps accepted game presents. Existing 30/120 FPS profiles remain readable,
with fixed 120 retaining its previous 1080p surface limit. Variable FPS is
bounded by the encoder's macroblock throughput: at most 240 captures/second
at 1080p and 144 at 2560×1440. GPU backpressure may drop frames. The source is
the launched game's Vulkan or supported desktop OpenGL presentation hook, not
the whole desktop or compositor scanout, and no picker appears. The same
Efficient, Balanced, and High quality presets apply to either mode before launch.
The output container remains selectable between MKV and
MP4 from Global settings or the in-game Replay menu; changes apply to future
saves without rebuilding the active buffer. The default is 60 FPS / Balanced.
Supported save lengths are 15/30 seconds and 1/2/3/5/10/15 minutes. The spool retains the
bounded 15-minute horizon; choosing a shorter manual save length selects a suffix,
not a smaller rolling horizon. Fixed 120 FPS is gated by display/game-surface
limits of 1080p or lower and actual encoder capability. Variable mode retains
bounded extra producer buffers and ring/spool space so faster bursts do not
shorten the requested history solely through the packet count cap.

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

The shortcut opens the Replay menu inside the captured game: the active Vulkan
or OpenGL capture backend renders the panel into the game's own presentation
target, so compositor stacking rules and fullscreen focus games cannot hide it
behind the game window. There is no
desktop menu window. The helper owns the pointer: while the menu is open it
grabs every mouse through the kernel so the game receives no pointer input, and
streams coalesced `MENU MOVE`/`MENU BUTTON` events over the replay control
socket; the daemon hit-tests them and answers `OK GRAB`/`OK RELEASE`. Escape,
any unrelated key (so Alt+Tab is never stranded), the helper's inactivity
timeout, and the daemon's command watchdog all release the mice and close the
menu. Capture teardown also closes the menu, and a subsequent pointer ping
receives RELEASE if the render target disappears. Some login sessions grant
keyboard access without granting raw mouse
access. In that case the menu still renders in a view-only fallback, the save
shortcuts remain active, and the shortcuts panel reports that pointer control
is unavailable; a transient kernel mouse-grab failure uses the same fallback
instead of cancelling the menu. Composite keyboard interfaces are evaluated in
one bounded chord window so modifier and function-key reader scheduling cannot
drop a configured chord. Pressing the assigned menu chord again or any unrelated
key closes it. The menu renders even
when the metrics overlay is hidden. Its bounded game-rendered surface and control
outlines use rounded corners, and measured labels are centered within their
cells so scaling cannot push shortcut or status text across a divider. The menu
follows the approved modern reference look: quiet letter-spaced labels above
bright values, thin dividers, a dark-red selected duration cell with a red
underline, and a red-outlined save button on a near-black rounded panel. The menu
shows the current menu chord and the selected duration's direct-save chord from
live persisted preferences; cleared bindings show as unassigned. Its format
picker persists MKV or MP4 and updates the active save runtime. A replay menu
requires a running captured session, and without one the helper reports the rejection and the shortcuts
panel shows it. The app does not expose a desktop preview because the production
menu is rendered by the capture backend inside the captured game.

A completed save drives the bottom-left **Moment saved** pill in the game,
including duration and Local library. It is independent of metrics visibility.
Do not show success on key press alone: completion must come from the native
save lifecycle. Buffer readiness, saving, unavailable, and failure are distinct.

## Capture and encoding

The production path uses private game-owned graphics exports, not KMS or desktop
screen scraping. Vulkan supports its implemented 8-bit and packed 10-bit
swapchain paths. On the validated AMD/RADV host, desktop OpenGL uses a fixed
six-slot pool of linear GBM RGBA8 buffers per bounded context, imported through
`GL_EXT_memory_object_fd`;
fence-ready descriptors pass to the same daemon-owned GPU conversion and
hardware H.264 worker. EGL/OpenGL ES Replay remains unsupported. Every route
still depends on format, dimensions, device, queue, driver, and encoder gates.
With Beta access enabled at startup, a single-render-node NVIDIA system may
attempt the same Vulkan Video H.264 path. The driver must expose the required
Vulkan Video encode, external-memory import, and queue capabilities. Multi-GPU
NVIDIA systems are withheld until Redunar can match the game's render GPU to the
encoder. An eligible NVIDIA attempt selects only a NVIDIA Vulkan device; the
ordinary AMD route remains separate. This is an unverified beta path, not an
NVIDIA recording guarantee.
When Debug log was enabled before launch, NVIDIA encoder-start failures add
bounded `NVIDIA Replay` lines with an allowlisted stage and reason code and,
where available, a numeric Vulkan result. These lines omit game names, paths,
device names, PCI addresses, UUIDs, and process IDs. The general logger still
prefixes each line with an absolute timestamp, and other lines may contain
session details. Ask testers to share only the relevant `NVIDIA Replay` lines
and remove their timestamp prefix if they prefer. Logs stay local until the
owner explicitly shares them.

Backpressure drops replay work rather than waiting for an encoder on the game's
presentation path. Resize starts a fresh codec epoch: completed old-generation
copies are discarded locally, already-exported buffers remain release-gated,
and the replacement source is announced only after its bounded pool exists.
Context destruction closes producer ownership while transferred DMA-BUF
duplicates remain valid in the daemon. Audio or recorder
failure must not stall the game. Recording has no software-video fallback;
FFmpeg's role in playback/export below is separate from live capture.
Failed producer-release acknowledgements are retained for retry on later pump
turns, including turns with no new frame. Audio operational messages report
only the allowlisted backend and transition, without output-device names.
If the daemon exits, the game continues presenting; telemetry batches reset
after failed sends and exported OpenGL slots remain bounded while release
messages are unavailable. A restarted daemon starts a new capture session,
so Replay requires a new Redunar game launch rather than attaching the old
game process to a different session. Startup sweeps stale private capture
socket directories.

When a process presents from multiple OpenGL contexts, the largest current
viewport owns metrics and Replay. Context destruction clears that choice so a
replacement context can take over immediately. OpenGL renderer and diagnostic
readback bookkeeping also relinquish the destroyed context's bounded slots.

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

`crates/redunar-capture-audio/src/source.rs` prefers the native PipeWire route:
when the PipeWire registry exposes a default output node, Redunar records it
directly with `pw-cat`. Recording through pipewire-pulse's compatibility layer
instead was observed on 2026-09-24 to disturb a game's own Pulse client
(Stardew's music went silent while a Pulse monitor recorder was attached during
a Redunar session and returned once the recorder was removed), so the Pulse
monitor API now serves as the fallback for hosts without a usable native
PipeWire route; it still works with a native PulseAudio server and with
PipeWire's Pulse server. Both paths produce fixed 48 kHz
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
