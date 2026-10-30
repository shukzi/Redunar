# Steam startup and automatic exit — October 9, 2026

This records the initial implementation. Owner testing later found that automatic
compositor focus retained the tray-disabled owner. The owner chose temporary tray
startup and live-game window parking; the [follow-up evidence](2026-10-09-steam-temporary-tray.md)
records the current behavior and build. Initial passes below remain historical.

Branch `fix/steam-background-auto-exit`, base
`ea6fa3bf85ffa8a52b8684ca02c4ad5a0e25769c` (v0.1.18). See the
[task brief](2026-10-09-steam-auto-exit-task.md).
Full validation and the offline production build used identical source manifests:
`9f398cf8d8b2fc3a1bb53b98bd3147aa596b92ab3c4f9e6c0154d52d8dd2b40a`.
This report and its documentation links were added afterward; production code
and tests did not change.

## Behavior and ownership

The owner requested automatic Steam startup to respect Close to tray consistently.
Enabled starts with the hidden main window and native tray; disabled creates no
tray item and requests a minimized taskbar window without activation. Missing
startup tray registration uses the same window fallback. Saved geometry is
restored before the first map, and startup does not rewrite preferences.

The native supervisor emits completion after natural process exit and successful
session cleanup/history recording. An automatic owner exits only while Close to
tray is disabled and the main window has never been opened/activated. Existing
manual owners and opened windows remain running. Pending cleanup/history or a
completion-time preference read error reveals the window for attention.

Completion carries a session generation. Shutdown is claimed under the session
lock, preventing stale completion from quitting a newer game. Callbacks release
ownership locks before scheduling GTK; shutdown removes the callback's AppHandle
reference before joining the supervisor. No polling worker, persisted flag,
dependency, permission change or data migration was added. Replay saving and
shutdown retain their existing cancellation policy; this change adds no save-drain
guarantee.

## Verification and review

- `tools/validate.sh full` passed both Cargo workspaces, all enabled Rust/Node/Python
  suites, strict Clippy, formatting and the production frontend build. The Tauri
  binary suite passed 109 tests, with two existing manual tests ignored.
  Report: `.redunar-build/reports/20261009T144406Z-full.wfcs7S.log`.
- Six new supervisor regressions cover completion before desktop attachment,
  callbacks outside engine/observer locks without UI polling, stale/duplicate
  completion, launch/shutdown arbitration, End while a game remains live, current
  preference reads, failed history recording and explicit retry. Existing
  authenticated Steam-wrapper natural-exit coverage now checks completion delivery.
  Startup state tests cover manual owners and sticky retention after Open.
- Built `steam_background_probe` and `desktop_controls_probe` offline in the
  checkout-owned Tauri check target. `python3 tools/check-steam-background.py`
  passed both tray-disabled and missing-host cases on private D-Bus/Xvfb sessions.
  They exercise production startup and activation calls, verify no tray item when
  disabled, fallback reachability, unchanged preferences, retention after Open,
  and that an existing fixture window keeps actual focus. Reports:
  `steam-background-tray-off.log`, `steam-background-tray-fallback.log` and
  `steam-background-probe-build-final.log` under `.redunar-build/reports/`.
- The existing desktop-controls probe passed hidden-window shortcut delivery,
  cleared shortcuts, tray startup and three registration/removal cycles on a
  separate private bus/display. Report: `steam-background-desktop-controls.log`.
- The production WebKit workspace fixture passed 97 checks in an isolated display.
  Settings and compact Steam setup screenshots were visually inspected: labels,
  preference controls and setup options remain readable. Report:
  `steam-background-workspace-webview.log`; images: `target/ui-review/`.
- `tools/build-linux-release.sh` passed using the cached offline Debian 12 image.
  All six runtime artifacts require at most glibc 2.34, below the 2.36 ceiling.
  Report: `steam-auto-exit-release.log`; manifest/baseline:
  `.redunar-build/linux/`. Built app SHA-256:
  `0fe179de3e69d68efe0cf9d576a80d0de802c7f43214c8cdfe85c07978b6af81`.
- `tools/stage-tauri-package.sh` passed in a fresh private staging root. All six
  staged runtime artifacts matched the production binaries byte-for-byte and had
  mode 0755 without setuid/setgid bits. No package was installed.
- A separate final review traced startup identity/geometry, production window
  activation, current preference reads, native completion, stale-event rejection,
  process supervision, retry and final shutdown. No persistent reader changed.
  Earlier iterations exposed a callback type-complexity lint and fixture-only
  focus/display assumptions; those were corrected before the passing final run.

## Remaining acceptance and recovery

Xvfb has no window manager: these fixtures establish focus requests and native
activation, but cannot establish actual compositor minimization or taskbar
presentation. The separately authorized installed-app Steam matrix in
[TESTING](../../TESTING.md#steam-play-entry-point) remains outstanding, including
tray-enabled hidden startup and natural exit after opening the window.
No installation, real-game run, version bump or publication occurred for this
change. The build remains v0.1.18 and does not replace the installed app.
Reverting the focused implementation requires no settings or recording migration.

## Authorized local installation

Later on October 9 the owner requested local testing of this version. The local
native Arch package was built from the verified production artifacts above:
`.redunar-build/steam-auto-exit-test-package/redunar-app-linux-x86_64.pkg.tar.zst`,
SHA-256 `8e4073302ca377741e4129fbbb89a5ec22b8d6a74cceadd1214586d559ad77ee`.
Its six runtime payloads match the compatibility build exactly. This test package
uses 0.1.18-1; it is not a newly published release.

No game process was active. The older app and shortcut helper exited via the
native tray Quit action. Desktop polkit authentication authorized the local
`pacman -U --noconfirm` installation, upgrading the installed package from
0.1.17-1 to 0.1.18-1. All 29 installed package files match the local payload;
both helpers are root-owned mode 0755 without setuid/setgid bits. Install log:
`.redunar-build/reports/steam-auto-exit-local-install.log`.

A fresh normal-user background app (PID 781000) and shortcut helper (PID 781037)
loaded the installed test executables. The app is owned by the user manager,
started with `--steam-background`, and uses the unchanged enabled Close to tray
preference. On the owner's niri/Wayland desktop, its native tray item registered
with the current tray host; no main window was mapped and the focused window
remained unchanged. Startup record:
`.redunar-build/reports/steam-auto-exit-installed-startup.json`.

This confirms enabled-tray startup on this desktop. The owner still needs to
check natural game exit, tray-disabled startup/automatic exit, and retention
after opening the window. No game was launched by the agent or new release
published in this follow-up.
