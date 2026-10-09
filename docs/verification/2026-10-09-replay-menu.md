# Replay menu pointer lifecycle — October 9, 2026

Branch `fix/replay-menu-dismissal`, base `9a3be61`. See the
[task brief](2026-10-09-replay-menu-task.md).
Full validation and the final offline compatibility build used the same source
manifest, SHA-256
`789cf4f9b8292249c5c4f110bdd30315902209297a44db5b415fc819b697d81d`.
This summary and its index links were added afterward; production code/tests
did not change.

## Findings and change

The owner reports Alt+Shift+Z briefly unfolding the menu before it disappears,
with recording and direct save shortcuts still working. Read-only inspection
of the running 0.1.17 helper on CachyOS found three mouse handles, two referring
to removed input nodes. Live menu telemetry was closed with Buffering and save
readiness. No control command or keyboard/pointer event was injected.

The old helper retained mice from startup. A failed grab entered view-only mode,
but normal polling still read those same mice and closed the menu on a read
error. Removed nodes explain this failure path; the exact live syscall failure
was not traced. A second bug let queued gameplay input reach menu controls.

The helper now refreshes mouse discovery before opening, consumes queued input
after taking all pointer grabs, and polls mice only in an owned pointer session.
View-only fallback retains heartbeats. Acquisition/read/cleanup-limit failures
release all mice; the next opening can discover current devices again. Existing
Escape, unrelated-key, repeat-toggle, save, inactivity, control failure and
capture-teardown behavior stays intact. No preference, persistence, wire ABI,
renderer styling, dependency or permission change.

## Checks and review

- `tools/validate.sh full` passed both Cargo workspaces, 22 helper tests, strict
  Clippy, formatting, Node/Python suites and the production frontend build.
  Report: `.redunar-build/reports/20261009T093041Z-full.mSPkZO.log`.
- Fake mice cover stale clicks/motion, reopening, fresh events, failed grabs and
  reads, and continuous-input cleanup limits. Private socket tests cover
  view-only heartbeats without pointer reads and normal owned-button delivery.
  The real keyboard monitor fixture covers the owner's Alt+Shift+Z with duplicate
  composite edges and modifier releases; discovery is fake and runs only on open.
- Earlier iterations exposed fixture syntax, closure lifetime and strict-lint
  errors; all were corrected before the final passing run. No gate was weakened.
- `tools/build-linux-release.sh` passed offline using the cached Debian 12 image.
  All runtime artifacts require at most glibc 2.34, below the 2.36 ceiling.
  Report: `.redunar-build/reports/replay-menu-release-final.log`.
- Existing CPU reference renderers for Vulkan/OpenGL generated the menu surfaces;
  both were visually inspected for intact layout, labels and controls. These
  images verify geometry, not live game presentation or pointer acquisition.
- Local RPM and native Arch package builds passed staging/payload/permission
  checks. Arch used its cached pinned image with networking disabled. Its warning
  about an absent package database is expected for `makepkg --nodeps`; it does
  not validate installed runtime dependencies. Reports: `replay-menu-rpm.log`
  and `replay-menu-arch-package.log` in `.redunar-build/reports/`.
- Separate final review followed native bindings through helper acquisition,
  event dispatch, daemon menu hit-testing/telemetry and both renderers, including
  failure release, reopening and shutdown. Save handling and stored data are
  unchanged. At the end of build qualification, the installed app/helper had
  not been replaced; the separately authorized installation is recorded below.

## Local test artifacts and remaining acceptance

Local test packages retain **0.1.17**; they are not a qualified new release.
Artifacts live in `.redunar-build/replay-menu-test-package/`.

| Artifact | SHA-256 |
| --- | --- |
| `redunar-app-linux-x86_64.pkg.tar.zst` | `35a2fb7a12c3fd904903b6e7646d901dade4204b74f3d35757393236c23d1c09` |
| `redunar-app.rpm` | `f862b2d1590fca88ac85e659deffe769490c349421e2c3b27e80fc0f29ab6d59` |
| Packaged `redunar-hotkey-helper` | `055bbcb569da561f85187364d2dc05deed04d66e221e4835a58608176575425f` |

Before installation, the running helper had SHA-256
`9464b1077d90cf2ec3c250437ff9afdebaca8b16e36c3e7254320a1a984f24a5`.
No agent-launched real game, GPU acceptance or publication ran.
After fresh app/helper startup, the owner must verify opening, pointer
interaction, close/reopen, saving and normal cleanup
in the affected game. A working result can proceed to a new version and exact
candidate qualification under [RELEASING](../RELEASING.md). No new vendor or
distribution support is claimed. Reverting helper changes needs no data migration.

## Authorized installation follow-up

On October 9 the owner confirmed the game was closed and authorized installation.
The old app/helper had already exited. Desktop polkit authentication authorized
`pacman -U --noconfirm` with the exact Arch test package above; reinstalling
0.1.17-1 completed successfully. No new release version was installed.

All six installed runtime executables/libraries and the update policy, desktop
entry, AppStream metadata, icon and input rule match that package byte-for-byte.
Both helpers are root-owned mode 0755. A fresh normal-user app (PID 618061)
started its fresh helper (PID 618102); their loaded executable hashes match the
package. The helper has no removed input handles and defers mouse acquisition
until menu opening. Report:
`.redunar-build/reports/replay-menu-installed-arch.txt`.

The manual test launch uses an empty `REDUNAR_UPDATE_SOURCE_URL` to avoid external
update requests for this session; persisted update preferences are unchanged.
After this installation, the owner reported that the in-game menu works. This
confirms the requested menu check on the installed test build; it does not add
broader Replay, reconnect-during-menu, vendor or distribution acceptance.
New-version qualification/publication is recorded separately when complete.
