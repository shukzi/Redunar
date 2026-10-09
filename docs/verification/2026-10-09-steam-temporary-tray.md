# Temporary Steam tray and live-game window close — October 9, 2026

Branch `fix/steam-background-auto-exit`, base
`ea6fa3bf85ffa8a52b8684ca02c4ad5a0e25769c` (v0.1.18). The owner reported
that the initial tray-disabled owner remained running after game exit on niri/DMS;
enabled Close to tray correctly retained the hidden owner. They chose temporary
tray startup regardless of the saved preference, and requested the same behavior
when closing an opened window during an owned game. See the
[task brief](2026-10-09-steam-auto-exit-task.md), [DESIGN](../../DESIGN.md)
and [ARCHITECTURE](../../ARCHITECTURE.md) for current policy and ownership.

Full validation and the offline production build used the identical source manifest
`839e1257d7c2ace1a1bfff1bbf43ac0f44915b8b6cf5480ad4131f0d6a65a69a`.
This report, its links and a clarification of DESIGN's temporary-icon exceptions
were added afterward; production code, tests and Settings text did not change.

## Implementation and review

Automatic Steam startup keeps the main window unmapped and registers a temporary
tray entry without changing preferences. Open removes that entry if Close to tray
is off and retains the foreground owner. Closing during a live owned game restores
the entry and relinquishes foreground retention, including for a manually started
owner. Off exits after natural game completion and successful cleanup/history;
on keeps the tray owner. Reopen cancels pending close/exit; close again re-arms it.
Idle close and explicit Quit retain their existing behavior. Window parking never
ends the session or Replay, and Quit never kills the game.

Native button/key/touch/scroll input retains deliberate use; focus and passive
map/pointer events do not. A private native focus-only probe reproduced the old
retention failure. Unavailable startup registration uses an unfocused window
fallback; unavailable live-game parking keeps the window visible with an error.
The fallback removes an off preference's temporary icon only after actual GTK
mapping, without interpreting its own recovery as Open. A weak map observer and
state-only input callbacks avoid AppHandle cycles. Close begins in a GTK idle
callback because Tauri's main-thread dispatch can run inline before the close
button's trailing input observer. Close tickets reject canceled/duplicate work.

Separate final review traced manual and automatic setup, native input, tray
registration/removal/recovery, actual owned-game completion, current preferences,
cleanup/history retry, generation rejection and atomic new-launch/shutdown claims.
Callbacks execute outside supervisor locks and are released before shutdown.
No persisted schema, runtime dependency, permission or save cancellation policy
changed. Fixture XTest calls use an existing system library only on private Xvfb.

## Verification

- `tools/validate.sh full` passed both Cargo workspaces, formatting, all enabled
  Python/Node/Rust suites, strict Clippy and the production UI build. The Tauri
  binary suite passed 112 tests, with two existing manual tests ignored. Report:
  `.redunar-build/reports/20261009T155955Z-full.A5dgq9.log`.
- `python3 tools/check-steam-background.py` passed all 12 private D-Bus/Xvfb modes:
  both tray preferences and unavailable-host startup, forced focus without input,
  real webview key/click input, Close/Open/Close, manual-owner parking, unavailable
  registration and pending-close cancellation. The parking fixture models the
  close button's trailing input observer. Report: `steam-window-close-native.log`
  and individual `steam-background-*.log` files in `.redunar-build/reports/`.
- Supervisor fixtures cover live ownership after End, failed history and explicit
  retry, completion before GTK attachment, delivery outside locks without UI
  polling, stale/duplicate generations, current preferences and launch arbitration.
- The rebuilt desktop-controls probe passed hidden-window shortcuts, cleared
  shortcuts and three registration/removal cycles. The production WebKit fixture
  passed 97 checks; the new Settings explanation was visually inspected with no
  overflow. Reports: `steam-temporary-tray-desktop-controls.log` and
  `steam-temporary-tray-workspace-webview.log`; screenshot: `target/ui-review/settings.png`.
  One concurrent Xvfb allocation failed before running a case; the final native
  runner allocated its displays sequentially and passed all cases.
- `tools/build-linux-release.sh` passed with the cached offline Debian 12 image.
  All six artifacts require at most glibc 2.34, below the 2.36 ceiling. Report:
  `steam-temporary-tray-release.log`; baseline/manifests: `.redunar-build/linux/`.
  App SHA-256: `7556395098a1547f54765d269aab355390728fe111740b116b15aa69a5d41037`.
- Native Arch package staging/build passed offline. All six package runtime
  payloads match the verified production artifacts and have mode 0755 without
  setuid/setgid bits. Package: `.redunar-build/steam-temporary-tray-test-package/redunar-app-linux-x86_64.pkg.tar.zst`,
  SHA-256 `0159f5d20e8290365a332e962d3f910bde6b5aef367597cd817fbca6f368af23`.
  This retains version 0.1.18-1 for local testing; no publication occurred.
- Desktop metadata validation and AppStream validation without network passed.
  All 66 local links in affected Markdown documents resolved; `git diff --check`
  passed. A final source-manifest comparison found only the documented Markdown
  additions/clarification after the validated production build.

## Local acceptance

The owner authorized replacing the earlier local test build. The previous app and
helper exited via native tray Quit after checking no game was running. Desktop
authentication subsequently completed and the local 0.1.18-1 package was reinstalled.
All 29 installed files match the package, including the app hash above; both
helpers are root-owned mode 0755. No fresh app process was running at verification,
so installed-process/game acceptance remains outstanding. Install log:
`.redunar-build/reports/steam-temporary-tray-local-install.log`.

Xvfb cannot establish real compositor minimization or game capture. The owner still
needs to run the [Steam matrix](../../TESTING.md#steam-play-entry-point), especially
Close to tray off: hidden automatic startup, Open removing the temporary icon,
close during a live game restoring it, and game exit removing it with the app.
The agent did not launch a real game. Reverting needs no data migration.
