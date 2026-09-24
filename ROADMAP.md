# Redunar current work

Reviewed September 24, 2026. Redunar has an implemented Tauri application;
release qualification remains incomplete. The package specification
is the build-version authority, not a version number copied into every guide.

## Implemented product

- The production Tauri workspace with Overview, Library, Global settings,
  Instant Replay, History, and Settings, backed by shared Rust services.
- Local game records, installed Steam/XDG import, direct/native Steam launch,
  explicit profile inheritance, cached poster/banner copies, and unavailable
  launch state for uninstalled games.
- Read-only hardware metrics, Vulkan frame telemetry, recorded session history,
  raw timeline inspection, and an existing same-game comparison view.
- Direct and native-Steam GLX/EGL/SDL2 OpenGL frame telemetry and bounded
  Compact, FPS only, Detailed, and Custom metric layouts through a private
  interposer. On the validated AMD/RADV host, desktop GLX and SDL OpenGL also
  use fixed-pool GBM exports and the existing Vulkan Video H.264 pipeline for
  30/60 FPS Replay. Synthetic direct/native-Steam acceptance creates playable
  clips with zero transport drops/rejects. The production session pump also
  commits decoded 1080p30/MKV and 1080p60/MP4 clips with H.264 and Opus.
  The in-game Replay menu and Moment saved surfaces are implemented; owner
  real-game acceptance completed on September 24, 2026 and the completion
  contract closed in [OPENGL-COMPLETION.md](OPENGL-COMPLETION.md), including
  the owner's waiver of the remaining full-matrix scope.
  Synthetic X11 fullscreen/restore and installed Fedora payload checks on an
  earlier candidate passed. The newest candidate also passed installed-payload
  and isolated fresh-process window checks; owner real-game acceptance remains.
  EGL/OpenGL ES remains metrics-only.
- Prepared in-game metrics with reversible visibility; automatic capability-gated
  replay, optional shortcuts, and independent Moment saved feedback.
- Hardware video capture, bounded rolling spool, local saves, native clip
  preparation/playback, filmstrip trimming, separate export, and deletion.
- Immediate Close to tray icon behavior and a dedicated replay-menu document.
- Local RPM/DEB/portable asset construction, a checksum-verifying distribution
  installer template, and automated native/frontend checks.

These are implemented paths, not blanket hardware or release certification.
[HARDWARE-SUPPORT.md](HARDWARE-SUPPORT.md) defines the narrower validated scope.

## Open correctness and release work

1. **Complete the public update experience.** The native signed
   metadata checker now selects a supported package, downloads it with bounds,
   verifies its checksum, retains it in a private cache, and exposes a native
   installer handoff; release builds embed the HTTPS GitHub release channel and
   Settings controls are implemented. Release assets now include an authenticated
   version, and the native host distinguishes an installed package from a
   cancelled or unconfirmed handoff through read-only package queries. An offline
   Fedora container covers RPM upgrade and rollback with both a synthetic
   package and an isolated genuinely newer Tauri binary. The Fedora 44 host
   has also completed a real-binary 0.1.3→0.1.4→0.1.3 upgrade/rollback,
   restart verification, and uninstall/reinstall without removing user state.
   A local signed-feed end-to-end test covers native download, replacement, and
   tamper rejection. Complete the public signed in-app download and desktop
   installer cancellation flow during early distribution. The webview must not
   gain package or shell authority.
2. **Resolve the reported Stardew playback failure.** Resolved on September 24,
   2026 with build reference redunar-app-0.1.3-9.local.fc44. The September 23
   symptom (no sound through Redunar's native Steam wrapper while direct Steam
   stayed audible) was isolated on September 24: a Steam launch carrying only
   the OpenGL sidecar in LD_PRELOAD ruled the sidecar out, and diagnostic RPM
   0.1.3-8 (Replay's host-side default-output recorder suppressed, everything
   else unchanged) restored music, metrics, and Replay. Discovery had preferred
   the PulseAudio monitor route on this PipeWire host; recording through
   pipewire-pulse's compatibility layer disturbed Stardew's own Pulse client.
   The fix prefers the native PipeWire default-output node with the Pulse
   monitor retained as fallback, and the owner validated music, effects,
   metrics, Replay, and clip audio together in one Redunar launch. See
   local-builds/pipewire-preferred-audio-capture-2026-09-24/STATUS.txt.
   Preserve the documented output-monitor fallback and make no game-only
   isolation claim.
3. **Use issue-driven cross-distribution qualification.** The Fedora RPM still
   requires the documented compatible codec provider. Additional distribution
   provider and dependency problems are followed up as users report them.
4. **Repeat affected installed-runtime acceptance.** Verify matching binary and
   sidecars and a fresh process before attributing behavior to a new package.
   Focus on replay-menu transparency/focus, visibility while recording, tray
   changes, cleared shortcuts, media preparation errors, and numerical history.
5. **Use issue-driven game and environment qualification.** Retained reports
   cover ARC Raiders and PEAK on the development Fedora/AMD host. Counter-Strike
   2 and other environments can be tried after publication; reports should
   drive fixes and later validation rather than blocking the initial release.
6. **Track performance reports after launch.** Retain the existing bounded
   failure, resize, disk-pressure, restart, and exit checks, while treating
   long-session overhead as follow-up evidence gathered from real use.
Keep source comments and error strings aligned with current behavior. Current
normative docs take precedence over dated evidence, while the owner's decisions
take precedence over both. Do not hide a behavioral disagreement by changing
only its description.

## Future desktop Replay

Add a selectable Desktop or Freestyle source for Instant Replay. Its goal is to
record the fully composed desktop as the user sees it, including the effects of
window placement and the desktop compositor, instead of capturing only a game
render target. Explore frame-rate and quality controls that can follow a
high-refresh display rather than inheriting the current game Replay FPS limit;
the achievable cadence must be measured and capability-gated. Decide later
where the source choice belongs in global/per-game settings, how multiple
monitors and audio should work, and which Linux desktop capture interface can
provide the required fidelity with explicit user consent. This is future scope,
not a claim that desktop capture or uncapped recording is implemented today.

## Scope discipline

Maintain the current replay, metrics, overlay, catalog, and history flows. Keep
the existing comparison view without implying an automated before/after workflow.
Library discovery is installed-entry based; native process supervision remains
necessary for launch ownership.

Broader launcher integration, Flatpak Steam capture, NVIDIA/ARM support, OpenGL ES Replay,
late attachment, desktop Replay, KMS production capture, cloud metadata, accounts,
and broader public releases are outside the current milestone. They require explicit
scope decisions and validation, not placeholder controls.

The single-command installer framework now detects Fedora/RHEL, Debian/Ubuntu,
Arch, and openSUSE families, authenticates a signed checksum manifest, and selects
a native RPM, DEB, or Arch package. Release binaries build in a pinned Debian 12
environment with a glibc 2.36 floor. The public source repository is
`shukzi/Redunar`. Publishing the source does not certify binary releases.
Release work still needs GitHub asset automation and in-app signed update
qualification on the initial Fedora release target.
Preserve existing compatible multimedia providers, use each platform's package
manager for required dependencies, and never silently replace codecs or add
third-party repositories. Distribution detection alone does not establish
app/runtime compatibility.

Use [TESTING.md](TESTING.md) and the
[real-game acceptance checklist](output/tauri-redunar/REAL-GAME-ACCEPTANCE.md)
for verification. Prior milestone claims are not current release evidence.
