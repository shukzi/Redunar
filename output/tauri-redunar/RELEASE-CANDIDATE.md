# Tauri release qualification

Reviewed September 20, 2026. The active local app is implemented, but release
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

- [x] Publish the signed `VERSION` metadata consumed by the in-app updater and
      clear ended sessions from the live Overview after startup while retaining
      History. The full in-app updater qualification remains open for installer
      result handling, user-data preservation, offline/failure/restart states,
      and keeping package privileges outside the webview. Signed metadata
      verification, supported-package selection, bounded download, and checksum
      verification, private cache persistence, and native installer handoff are
      implemented. Release builds now embed the HTTPS GitHub release channel;
      development builds remain source-pending unless given an isolated test
      feed. Remaining work is installer result handling, user-data preservation,
      offline/failure/restart states, and keeping package privileges outside the
      webview.
- [x] Retain owner-approved output-monitor fallback; document mixed audio honestly.
- [x] Add FFmpeg/FFprobe file and complete-codec capability requirements without
      pinning or replacing a compatible multimedia provider.
- [deferred] Validate intended audio sources and media-provider integration on
      additional release environments. The initial release must describe the
      output-monitor fallback honestly and make no game-only audio claim.
- [x] Pass the relevant automated/native/package gate on the final source.
- [x] Verify installed binary/sidecars match and test a freshly opened process.
- [ ] Repeat unaffected visibility, optional-hotkey, tray, playback, and History
      checks with the final runtime. The changed Replay menu passed owner
      acceptance on the package built immediately before the v0.1.3 bump; the
      final-source titlebar, resizing, and scroll boundary passed owner
      acceptance before the exact package was built and installed.
- [deferred] Complete the Counter-Strike 2 acceptance run.
- [deferred] Validate another Linux host/distribution's package and embedded
      WebKit playback, seeking, export, and desktop integration.
- [x] Build release artifacts in the Debian 12/glibc 2.36 environment and sign a
      manifest covering every package asset.
- [ ] Validate installation, upgrade, uninstall, and runtime behavior on the
      one Fedora 44 environment claimed for the initial release.
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
