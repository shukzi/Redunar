# Tauri release qualification

Reviewed September 24, 2026. The active local app is implemented, but release
qualification is still open. Read `packaging/redunar-app.spec` for the current
package version; do not infer the installed or running version from this file.

## What the existing evidence establishes

Retained reports describe owner-tested ARC Raiders and PEAK flows on the local
AMD/RADV host, plus isolated capture, codec, native bridge, and UI checks.
Their exact build/environment and limits matter. These historical passes do not
certify subsequent changes, other GPUs, another compositor, or a fresh package
installation.

Current design/calculation contracts are [DESIGN](../../DESIGN.md) and
[History calculations](HISTORY-CALCULATIONS.md).

## Current automated evidence

On September 23, 2026, the full offline Tauri release gate passed on an
isolated snapshot of the current working tree. The Debian 12 build stayed
within the glibc 2.36 baseline. The Tauri binary and native capture sidecars,
Rust tests, strict Clippy, frontend tests, desktop metadata, package staging,
and signed asset checks passed. The Fedora 44 updater RPM flow and the
isolated real-binary 0.1.3→0.1.4→0.1.3 upgrade/rollback test also passed.

The review bundle is under
[`local-builds/release-candidate-local-test-signature-2026-09-23`](../../../local-builds/release-candidate-local-test-signature-2026-09-23/STATUS.txt).
Its Fedora RPM SHA-256 is
`705e962cecf3c62ec31b6cafe11427e89e3ace627c6f8cf12794eb486398d247`.
Its `SHA256SUMS.sig` uses the bundle's test-only public key and is not a
production update signature. The candidate RPM's runtime files match the
currently installed payload. A fresh installed process passed all three
isolated 2× Xvfb window-size cases. No installation was performed during this
revalidation. `rpm -V redunar-app` reported only user/group ownership
differences (`nobody:nobody` on this host); candidate payload hashes and modes
matched.

On September 24, the production WebKit workspace fixture passed 75 checks and
the Replay fixture passed its scrolling, focus, media-readiness, decoded-frame,
and trim/export checks in isolated Xvfb. Visual inspection covered the
workspace and Replay fixture screenshots. This is synthetic UI evidence, not
owner acceptance of a real game or installed release.

The owner-reported Stardew Valley audio failure was resolved on September 24,
2026 with build reference redunar-app-0.1.3-9.local.fc44: Replay's
default-output recorder now prefers the native PipeWire route instead of the
Pulse monitor on PipeWire hosts, and the owner validated music, effects,
metrics, Replay, and clip audio together in one Redunar launch. See
local-builds/pipewire-preferred-audio-capture-2026-09-24/STATUS.txt and
[REAL-GAME-ACCEPTANCE](REAL-GAME-ACCEPTANCE.md) for the isolation chain and
[OPENGL-COMPLETION](../../OPENGL-COMPLETION.md) for the remaining
owner-controlled OpenGL acceptance gates.

On September 20, 2026, the full `tools/check-tauri-release.sh` gate passed for
the v0.1.3 source. It built the pinned glibc 2.36 compatibility artifacts,
passed the Rust, frontend, license, installer, staging, desktop metadata, and
package checks, and produced signed DEB, RPM, openSUSE RPM, Arch, and portable
artifacts.

This evidence applies only to the checked source snapshot. The passing gate does
not by itself qualify an installed package or a public release.

Earlier on September 20, 2026, the Fedora 44 RPM built from the accepted feature
source was installed over the existing package. Its package version was still
v0.1.2 because the v0.1.3 version bump followed owner acceptance. The installed
binary and sidecars matched the package, and a fresh process loaded without an
external GDK backend override. The owner confirmed the redesigned in-game Replay
menu, including its save action, format selection, and live buffer controls.

The final v0.1.3 Fedora 44 RPM was then installed from the qualified source.
`tools/check-installed-tauri-runtime.sh` reported matching application binary,
capture layer, Steam wrapper, shortcut helper, desktop entry, AppStream
metadata, icon, and uaccess rule. A fresh `/usr/bin/redunar-tauri` process
started without an external GDK backend override, remained running for ten
seconds, and terminated cleanly. Before packaging, the owner also accepted the
final-source compact app-owned titlebar, native edge/corner resizing, and the
content scrollbar beginning below the titlebar. This shell does not select or
require a particular compositor or GDK display backend.

On September 16, 2026, the v0.1.0 Fedora 44 RPM was installed on the
owner-controlled Fedora 44 x86_64 host. `tools/check-installed-tauri-runtime.sh`
reported matching application binary, capture layer, Steam wrapper, shortcut
helper, desktop entry, AppStream metadata, icon, and uaccess rule. A fresh
`/usr/bin/redunar-tauri` process then stayed running for 20 seconds with no
startup output and was terminated cleanly. This evidence does not establish
that the rebuilt initial-release package is installed or running.

## Owner-scoped initial release

The initial release will use an issue-driven qualification model. Fedora 44,
x86_64, AMD/RADV, ARC Raiders, and PEAK form the known tested baseline. Other
Linux distributions, games, WebKit environments, audio providers, and longer
session patterns may be used by early adopters, but are not represented as
completed validation. Compatibility will expand through real reports and fixes
after publication rather than requiring a full matrix before launch.

The deferred checks below are therefore follow-up work, not publication gates.

## Open release gates

- [x] Generate `VERSION` and a signed checksum manifest in the isolated release
      gate, and clear ended sessions from live Overview while retaining History.
      The native updater verifies signed metadata and package bytes, retains a
      bounded private-cache copy, and requests a desktop installer through a
      fixed native handoff. A local signed-feed test covers download, refresh,
      and tamper rejection with a temporary key. Release builds embed the HTTPS
      channel; development builds remain source-pending without a test feed.
- [deferred] Complete a production-signed in-app download and desktop installer run on
      the Fedora target. Verify cancellation/retry, offline and failure states,
      restart detection, user-data preservation, and package privileges staying
      outside the webview. The local signed-feed fixture, isolated RPM transactions,
      and native installer-state tests do not establish this GUI end-to-end gate.
- [x] Resolve the reported Stardew Valley no-sound launch through Redunar's
      native Steam wrapper. On September 24, the owner confirmed that build
      0.1.3-9.local.fc44 restored music, effects, metrics, Replay, and clip
      audio together. The recorder now prefers native PipeWire capture and
      retains the Pulse monitor fallback.
- [x] Retain owner-approved output-monitor fallback; document mixed audio honestly.
- [x] Add FFmpeg/FFprobe file and complete-codec capability requirements without
      pinning or replacing a compatible multimedia provider.
- [deferred] Validate intended audio sources and media-provider integration on
      additional release environments. The initial release must describe the
      output-monitor fallback honestly and make no game-only audio claim.
- [x] Pass the relevant automated/native/package gate on the final source.
- [x] Verify installed binary/sidecars match the current release candidate and
      test a freshly opened process. On September 23, the latest Fedora RPM
      matched every packaged runtime component. Three isolated fresh processes
      loaded `/usr/bin/redunar-tauri` and restored the expected window size at
      2× scale. On the preceding candidate, fresh process PID 598952 resolved to
      the installed executable and owned the Replay socket.
      Earlier apparent first-launch conflicts came from an older process
      retained across same-version reinstall.
      A deliberate secondary launch revealed a primary Tauri duplicate-webview
      panic; the secondary now uses a read-only service and non-unique GTK mode.
      A second installed process stayed open; the primary retained Replay
      socket ownership without a crash. Updater commands now check primary
      write access before changing the shared update cache.
- [ ] Repeat unaffected visibility, optional-hotkey, tray, playback, and History
      checks with the final runtime. The changed Replay menu passed owner
      acceptance on the package built immediately before the v0.1.3 bump; the
      final-source titlebar, resizing, and scroll boundary passed owner
      acceptance before the exact package was built and installed.
- [deferred] Complete the Counter-Strike 2 acceptance run.
- [deferred] Validate another Linux host/distribution's package and embedded
      WebKit playback, seeking, export, and desktop integration.
- [x] Build release artifacts in the Debian 12/glibc 2.36 environment and sign a
      manifest covering every package asset with a temporary local test key.
      Production-key signing and publication remain separate owner-controlled
      steps.
- [x] Validate installation, upgrade, uninstall, and runtime behavior on the
      one Fedora 44 environment claimed for the initial release. On September
      23, the host upgraded the actual 0.1.3 executable to an isolated 0.1.4
      build, restarted into it, rolled back, uninstalled, and reinstalled the
      earlier candidate. The Redunar user-state file count and byte total stayed
      unchanged, and the final payload and fresh process matched the candidate.
      The separate in-app signed-download and desktop-installer handoff remain
      open under the updater gate above.
- [deferred] Record representative runtime overhead and long-session evidence
      against the performance budgets. Keep the existing synthetic and bounded
      failure/resize checks, but do not claim a completed performance study.

[ROADMAP.md](../../ROADMAP.md) owns the rationale and scope;
[TESTING.md](../../TESTING.md) owns commands, and
[REAL-GAME-ACCEPTANCE.md](REAL-GAME-ACCEPTANCE.md) owns the manual checklist.
Record results with the tested build, host, date, pass/failure, and remaining
limits rather than replacing this checklist with an undated completion claim.

Local RPM/DEB/Arch/openSUSE/portable assets and a rendered installer passing these gates do not
authorize publication, new remotes, telemetry, website deployment, or GitHub
release creation. Those remain explicit owner decisions.
