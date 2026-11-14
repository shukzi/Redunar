# Redunar architecture

Redunar is a Tauri desktop application with a shared Rust service. The webview
renders state and requests typed commands. Native code owns sessions, hardware,
media, permissions, persistence, and cleanup.

## Runtime boundaries

```text
Tauri webview: output/tauri-redunar/ui/
    typed commands, displayed data, local drafts
Tauri host: output/tauri-redunar/src-tauri/src/
    commands, windows, startup, shutdown, supervision
Shared service: crates/redunar-daemon/ and crates/redunar-core/
    profiles, catalog, monitor, capture, Replay, history, persistence
Capture components: Vulkan layer, OpenGL interposer, audio, hotkeys, Steam bridge
    bounded same-user protocols and literal process arguments
```

The service runs inside the Tauri process. A separate D-Bus daemon is not
required. A second launch cannot take over an active session. Hidden webviews
do not stop native supervision or shortcuts. The webview cannot write hardware,
choose arbitrary playback paths, run privileged commands, or decide that
capture succeeded.

Native startup selects Wayland/X11 backend order before GTK initialization. An
explicit `GDK_BACKEND` remains authoritative. Steam forwards only allowlisted
desktop and audio environment values. The app does not rewrite the environment
passed to a game or helper.

A private state-directory lock and same-user activation socket coordinate
multiple launches. A secondary manual launch asks the primary to show its
window and exits before creating another GTK application. Steam background
launches remain hidden and use the same session coordinator. The socket accepts
bounded activation messages, not commands or arbitrary paths.

## Component ownership

| Component | Responsibility |
| --- | --- |
| `redunar-core` | Typed identities, profiles, measurements, capabilities, and compatibility models |
| `redunar-platform` | Linux telemetry, process adapters, launch planning, and Steam bridge |
| `redunar-daemon` | Catalog, preferences, monitor, capture receiver, Replay lifecycle, history, and storage |
| `redunar-capture-vulkan` | Vulkan frame measurements, overlay, frame export, and Vulkan Video admission |
| `redunar-capture-opengl` | Launch-scoped GLX/EGL/SDL measurements and guarded desktop-OpenGL export |
| `redunar-capture-audio` | Default-output and optional microphone capture with bounded Opus packets |
| `redunar-hotkeys` | Same-user keyboard and pointer helper |
| Tauri host | Typed commands, windows, tray, update handoff, supervision, and cleanup |
| Webview | Presentation, drafts, validation messages, and user requests |

The service keeps preference and catalog facade methods in private `service/`
modules. `RedunarService` retains its public methods and runtime ownership.
The UI's `replay-player.mjs` owns playback preparation requests, media listeners,
seek generations, trim gestures and disposal. The application passes its selected
filename and range to the export flow; native code still authorizes media paths.

The shared service coordinates capture and Replay state. Producers use
versioned private protocols, bounded queues, device identity, and release
acknowledgements. Device loss, process replacement, stale results, and cleanup
failure must not redirect data to a new session.

CPU power uses direct readable counters or the fixed installed
`redunar-cpu-power-helper` through polkit. The privileged endpoint accepts no
arguments or paths and emits only aggregate package power. The native package
policy authorizes active local sessions and denies inactive and remote
sessions. The sampler uses bounded nonblocking IPC, one outstanding request,
stale
response rejection, and a 30-second failure backoff. Disconnection ends the
helper. A failed child must be reaped before a replacement can start.

## Source map

Paths are relative to the repository root:

| Flow | Main files |
| --- | --- |
| Commands and app lifetime | `output/tauri-redunar/src-tauri/src/main.rs`, `output/tauri-redunar/src-tauri/src/backend.rs`, `output/tauri-redunar/src-tauri/src/runtime.rs`, `output/tauri-redunar/src-tauri/src/activation.rs` |
| Display selection | `output/tauri-redunar/src-tauri/src/display_backend.rs` |
| Startup, launch, and supervision | `output/tauri-redunar/src-tauri/src/launch_plan.rs`, `output/tauri-redunar/src-tauri/src/sessions.rs`, `output/tauri-redunar/src-tauri/src/session_lifecycle.rs`, `output/tauri-redunar/src-tauri/src/background_start.rs` |
| Catalog and profiles | `crates/redunar-daemon/src/service/catalog.rs`, `output/tauri-redunar/src-tauri/src/catalog.rs`, `output/tauri-redunar/src-tauri/src/profiles.rs`, `output/tauri-redunar/ui/game-drafts.mjs` |
| Installation and artwork | `output/tauri-redunar/src-tauri/src/installation.rs`, `output/tauri-redunar/src-tauri/src/artwork.rs`, `output/tauri-redunar/ui/game-artwork.js`, `output/tauri-redunar/ui/game-installation.js` |
| Playback and export | `output/tauri-redunar/src-tauri/src/playback.rs`, `output/tauri-redunar/src-tauri/src/clip_export.rs`, `output/tauri-redunar/src-tauri/src/media.rs`, `output/tauri-redunar/ui/clip-export.mjs`, `output/tauri-redunar/ui/replay-player.mjs` |
| Replay and shortcuts | `output/tauri-redunar/src-tauri/src/hotkeys.rs`, `crates/redunar-hotkeys`, `crates/redunar-capture-vulkan/src/overlay.rs` |
| Tray and lifecycle | `output/tauri-redunar/src-tauri/src/tray.rs`, `output/tauri-redunar/src-tauri/src/tray_native/`, `output/tauri-redunar/src-tauri/src/main.rs` |
| Updates | `output/tauri-redunar/src-tauri/src/updates.rs`, `output/tauri-redunar/src-tauri/src/runtime.rs`, `output/tauri-redunar/ui/app.js` |
| Preferences compatibility | `crates/redunar-daemon/src/service/preferences.rs`, `crates/redunar-daemon/src/app_preferences.rs`, `output/tauri-redunar/src-tauri/src/runtime.rs` |
| NVIDIA metrics | `crates/redunar-nvidia-nvml`, `crates/redunar-platform/src/linux.rs`, `crates/redunar-daemon/src/monitor.rs` |
| Replay encoding | `crates/redunar-daemon/src/replay_encoder.rs`, `crates/redunar-capture-vulkan/src/replay_video.rs` |
| UI/native data mapping | `output/tauri-redunar/ui/native-data.mjs`, `output/tauri-redunar/ui/app.js` |

Use [DESIGN.md](DESIGN.md) for interface behavior and [REPLAY.md](REPLAY.md)
for media behavior. Avoid duplicating those contracts here.

## Catalog, profiles, and launch

A game record contains a stable local ID, validated executable and arguments,
optional working directory, match rules, and optional launcher metadata. Steam
identity assists launch and artwork lookup; it does not replace the local game
record.

Installed-game import reads bounded Steam manifests and supported native XDG
entries. It is read-only until a user imports a candidate. Running helper
processes are not import entries. Ambiguous identity remains unresolved.

Global defaults resolve through inherited or custom per-game values. Saves retain
inheritance, reject stale drafts, and preserve unrelated pending edits. Missing
executables remain in the catalog and are rejected before launch preparation.

The native Steam bridge verifies the same-user wrapper, imported game identity,
and prepared profile. When the user service manager is unavailable, the game
launch remains usable and starting Redunar first can avoid the startup dependency.
The bridge never re-executes Steam's game arguments.

## Session lifecycle

1. Validate the game, installation, profile, and runtime capabilities.
2. Prepare the private capture runtime and optional Steam activation.
3. Spawn with literal arguments and an explicit working directory.
4. Supervise the authenticated game process and publish actual status.
5. Allow reversible metrics visibility without attaching to an unrelated game.
6. End the session, retain the launch lock until process exit, record history,
   and release owned resources. Failed cleanup retains its transport and blocks
   completion, another launch, and update restart until safe release succeeds.

Replay is requested only for supported capabilities. Metrics may remain available
when Replay is unavailable. End session does not kill the game; explicit Quit
ends Redunar after cleanup.

## Metrics, capture, and overlay

The shared service caches capture-runtime and encoder discovery for capability
reads. Routine diagnostics polling reuses that snapshot. An explicit diagnostics
refresh or capture retry requests fresh evidence. Explicit
invalidation triggers discovery on the next capability read.
Discovery, refresh and invalidation are serialized separately from live monitor,
session and Replay state, whose readings continue to update independently.

The monitor discovers sensors once, samples cached paths at 1 Hz, and publishes
bounded latest snapshots. Missing sensors affect only their own values. Frame
producers batch monotonic intervals over private transport and identify the
physical GPU and driver used for export. The encoder must match that identity.

Vulkan and supported desktop OpenGL producers feed the hardware H.264 path. The
OpenGL bridge uses bounded GBM/external-memory resources and device-specific
synchronization. NVIDIA uses a UUID-matched Vulkan bridge; AMD/RADV keeps its
existing path. OpenGL ES/EGL Replay, Flatpak capture, late attachment, and
software-video fallback are unsupported.

The active capture backend renders the overlay, in-game Replay menu, and
save notice. Overlay visibility is reversible and independent from the
metrics preset. The webview preview is a configuration surface, not the
in-game menu.

## Persistence and updates

State normally lives below `$XDG_STATE_HOME/redunar` or
`~/.local/state/redunar`. Catalog, preferences, history, artwork, Replay files,
and temporary playback files have separate bounded formats. Writes use private
temporary files, validation, atomic replacement, and directory synchronization.

Compatibility readers preserve existing profiles, recordings, and settings.
Format changes require old-state tests, migration behavior, and interruption
handling. Redunar does not add accounts, cloud sync, or telemetry as incidental
features.

Updater cache reads reject linked and special files and enforce byte limits.
Reserved temporary files use exclusive private creation and atomic replacement.
Interrupted owned temporary writes can be retried without following links.
The installer helper applies the same nonblocking file checks before copying
user-owned input into its private installation snapshot.

The updater verifies signed release metadata, version, and package checksums
before retaining a package. Settings can request a check or installation. The
webview cannot select a package path or command. Installation and restart
remain
separate operations.

## Privilege and failure boundaries

The UI and service do not run as root. Native privileged helpers use narrow
allowlists and fixed literal arguments. Redunar never changes firmware, voltage,
permanent kernel policy, or arbitrary system settings.

Every capability reports ready, unavailable, or failed state. Cleanup failures,
missing permissions, device loss, stale callbacks, and unsupported formats stay
visible and cannot silently become successful data.

## Validation

Use [TESTING.md](TESTING.md) for automated, package, capture, and installed-runtime
checks. Use [REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for native Steam runs.
Record exact source, build, package, hardware, driver, session, and limitations.
Do not claim hardware or real-game support from synthetic data.
