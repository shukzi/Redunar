# Redunar current work

Reviewed October 18, 2026 against `main` at `825012d` and the published
[v0.1.26 release](https://github.com/shukzi/Redunar/releases/tag/v0.1.26).
Public source and Linux packages are available. Version metadata is owned by
the package specification and checked against the native application metadata;
see [the release procedure](docs/RELEASING.md).

The latest published release improves cached clip thumbnails. Current `main`
also includes saved-clip refresh without restarting and trimmed exports that
can exceed the rolling capture buffer's size budget while retaining bounded
output sizes, free-space checks, and protection for existing recordings. These
source changes are not yet included in a published package release.

## Implemented product

- The production Tauri application provides Overview, Library, Global settings,
  Instant Replay, History, and Settings through shared Rust services.
- Local game records, installed Steam/XDG discovery, supported direct and native
  Steam launches, explicit global/per-game inheritance, and cached artwork.
- Read-only system metrics, frame measurements, recorded session history, and
  explicit comparison between sessions of the same game.
- Capability-gated Vulkan and supported desktop OpenGL Replay, an in-game
  metrics overlay with reversible visibility, optional shortcuts, and independent
  save feedback. The validated baseline is x86_64 Linux with AMD/RADV;
  [HARDWARE-SUPPORT.md](HARDWARE-SUPPORT.md) defines the narrower tested scope.
- Bounded rolling recording, local saves, clip browsing and playback, filenames
  and favorites, trimming to a separate export, and recoverable clip trash.
  System-output audio and optional microphone capture follow
  [REPLAY.md](REPLAY.md#audio-behavior), including the native PipeWire preference
  and PulseAudio-compatible fallback.
- Tray and background-launch behavior, single-instance activation, signed update
  checks, and a native package-manager installation handoff.
- A release workflow producing Fedora/openSUSE RPMs, Debian packages, native Arch
  packages, a portable archive, and signed checksums. The installer selects a
  package for the detected distribution family. Build and dependency details
  belong to [packaging](packaging/README.md).

## Current priorities

1. **Qualify the next release.** Include the newer source fixes in the next
   reviewed candidate and run the affected automated, packaging, and manual
   checks before publication. Keep evidence tied to the exact candidate;
   earlier release results do not qualify a later build.
2. **Follow reported game and environment failures.** Use bug reports to drive
   work on Steam activation, capture readiness, audio, playback/export, and
   session cleanup. Validate Wayland/X11 and distribution-specific dependencies
   separately. Successful discovery or packaging does not prove capture works.
3. **Verify lifecycle and update recovery.** Retain focused checks for repeated
   actions, cancellation, disk pressure, stale state, failed cleanup, and
   interrupted updates. Installed-runtime checks compare the exact package with
   the installed payload and confirm that a fresh process loaded it.
4. **Measure performance in real use.** Follow
   [PERFORMANCE.md](PERFORMANCE.md) for collection budgets and measurement
   procedures. Investigate reported long-session overhead without promising
   an FPS gain or treating synthetic results as hardware acceptance.

Use [TESTING.md](TESTING.md) and the
[real-game acceptance checklist](output/tauri-redunar/REAL-GAME-ACCEPTANCE.md)
for verification. Builds do not authorize installation, app restarts, or live-game
runs. Dated reports describe their recorded builds and hosts.

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

NVIDIA handling remains an opt-in beta path. Detection and in-game metrics have
reported tester evidence; successful NVIDIA recording remains unverified.
Neither those reports nor fake-hardware tests establish general NVIDIA support.

Broader launcher integration, Flatpak Steam capture, ARM builds, OpenGL ES Replay,
late attachment, production KMS capture, cloud metadata, and accounts remain
outside the current milestone. New scope requires an explicit decision and
validation, rather than placeholder controls.

Preserve compatible multimedia providers and use each platform's package manager
for required dependencies. Do not silently replace codecs or add third-party
repositories. Published packages and distribution detection do not establish
universal runtime compatibility.

Bug reports and feature suggestions are welcome; contributions to the official
repository are invitation-only. See [CONTRIBUTING.md](CONTRIBUTING.md).
