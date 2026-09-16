# Redunar current work

Reviewed September 16, 2026. Redunar has an implemented Tauri application;
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

1. **Complete the update experience before publication.** The native signed
   metadata checker now selects a supported package, downloads it with bounds,
   verifies its checksum, retains it in a private cache, and exposes a native
   installer handoff; release builds embed the HTTPS GitHub release channel and
   Settings controls are implemented. Complete installer result handling,
   user-data preservation, and offline, failure, cancellation, restart, and
   rollback states. The webview must not gain package or shell authority.
2. **Use issue-driven audio validation.** The initial release documents the
   approved output-monitor fallback honestly and makes no game-only isolation
   claim. Additional source/provider problems are handled from real reports.
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

## Scope discipline

Maintain the current replay, metrics, overlay, catalog, and history flows. Keep
the existing comparison view without implying an automated before/after workflow.
Library discovery is installed-entry based; native process supervision remains
necessary for launch ownership.

Broader launcher integration, NVIDIA/ARM support, OpenGL, late attachment, KMS
production capture, cloud metadata, accounts,
and broader public releases are outside the current milestone. They require explicit
scope decisions and validation, not placeholder controls.

The single-command installer framework now detects Fedora/RHEL, Debian/Ubuntu,
Arch, and openSUSE families, authenticates a signed checksum manifest, and selects
a native RPM, DEB, or Arch package. Release binaries build in a pinned Debian 12
environment with a glibc 2.36 floor. The public source repository is
`shukzi/Redunar`. Publishing the source does not certify binary releases.
Release work still needs GitHub asset automation and real installation,
upgrade, and uninstall validation on the one initial Fedora release target.
Preserve existing compatible multimedia providers, use each platform's package
manager for required dependencies, and never silently replace codecs or add
third-party repositories. Distribution detection alone does not establish
app/runtime compatibility.

Use [TESTING.md](TESTING.md) and the
[real-game acceptance checklist](output/tauri-redunar/REAL-GAME-ACCEPTANCE.md)
for verification. Prior milestone claims are not current release evidence.
