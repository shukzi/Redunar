# Native Steam Play capture verification — October 5, 2026

Task: [Steam Play entry point](2026-10-05-steam-play-task.md).
Branch `feat/steam-play-capture`, base
`1996c2c1f6470cae70a2c638739ed6701b6d551e`, package version `0.1.13`.
The full validation and compatibility build used source-manifest SHA-256
`947756c34b6701707eb29d3a64af38f1007984b81e1b6b831f1d7f5f3a8dc8bd`.
This evidence summary was added afterward without changing runtime code.

## Behavior and ownership

The existing app-specific Steam launch option can now request preparation from
the native session owner before exec. When needed, the wrapper starts its
packaged sibling app with `--steam-background`. The app uses the uniquely
imported Steam identity, effective inherited/custom profile, existing capture
activation protocol and Replay coordinator. The wrapper executes Steam's
original literal command once; the app does not launch that command again.

The app-lifetime listener verifies same-user kernel credentials and the wrapper
PID; the native callback verifies the packaged wrapper's executable inode.
Live wrapper evidence permits Steam option edits that have not yet reached its
saved configuration. App-initiated forwarding retains the saved-option check.
Requests cannot replace an active session or select an ambiguous/missing game.
External supervision reuses the daemon's bounded PID/start-time identity.
End retains the live game's lock, and history/cleanup use the same owner as
app-initiated sessions. Background startup uses a temporary reachable tray icon
or visible fallback without saving Close to tray.

No catalog/profile/preferences format or capture-wire version changed. No new
dependency was introduced, and no MangoHud implementation was incorporated.

## Verification

Ordinary tests used fake hardware, private fixture state and checkout-owned
build outputs on x86_64 Linux.

- `tools/validate.sh full`: passed both Cargo workspaces, Rust/Node suites,
  production UI build, syntax, formatting and strict Clippy. Report:
  `.redunar-build/reports/20261005T204428Z-full.QbcRlu.log`.
  The final repeat after adding this summary is retained as
  `.redunar-build/reports/steam-play-final-full-console.log`.
- Regression coverage exercises the actual wrapper's request/one-shot claim
  before exec, literal argv on denial, fake-app background startup, existing
  owner reuse, malformed/forged credentials, unique imported identity,
  duplicate-session rejection, PID reuse/disappearance, End retaining a live
  process, and one history record. Existing direct/forwarded launch, capture,
  failed-spawn, history-error and cleanup fixtures passed.
- Production WebKitGTK workspace fixture: 84 checks passed. The Steam setup
  instructions and the screenshot `target/ui-review/steam-play-setup.png` were
  visually inspected. Report: `.redunar-build/reports/steam-play-visual.log`.
  An earlier attempt stopped at the RAM-cleared polling assertion; the final
  production UI rerun passed without changing the RAM implementation/assertion.
- `tools/check-tauri-release.sh`: passed the offline Debian 12 compatibility
  build, glibc ceiling, license inventory, metadata, staging, local DEB/portable/
  Arch/openSUSE/Fedora package checks and test signatures. Report:
  `.redunar-build/reports/steam-play-package-console.log`.
  All runtime artifacts require glibc at most 2.34. RPM tools emitted warnings
  about this host's absent package database/transaction lock; package assertions
  and the gate still exited successfully. No package transaction was performed.

Built app SHA-256:
`30bab5f5ab9ccbaa0665ffbf8333d7a145f6b6331215370a85c03fab3bd792a1`.
Built wrapper SHA-256:
`0c92d170bacb3fa207d3e24e8e5d1311ecb11f368500de40ca0daf046c9dc609`.
The remaining sidecar hashes are in
`.redunar-build/linux/linux-release-baseline.txt`.

## Separate lifecycle and permission review

Reviewed the wrapper, native callback/coordinator, process supervisor, inherited
profiles, background window/tray, shutdown, bounded requests and socket cleanup.
The request contains only a fixed app ID/PID; the app accepts no game argv or
arbitrary paths. A denied request cannot spawn another app. Startup does not
carry Steam's game identity/library/capture overrides into Redunar. The primary
lease prevents a concurrent background starter from creating a second backend.
Quit closes admission before cleanup, and failed cleanup retains the existing
retry/visible-error path. No normal log gained a raw game path or process UUID.
No outstanding correctness finding remained in this review.

## Limits and recovery

No installation, live Steam/game run, NVIDIA recording, other-distro runtime or
publication was performed. The new live Steam entry point, desktop tray fallback
and NVIDIA save/playback still require the separately authorized acceptance
described in [TESTING](../../TESTING.md). Fixture success does not establish
successful encoding or support on the tester's RTX 3090 Ti.

Use a matching newly built app, wrapper and capture libraries, with saved Beta
access and Debug logging if testing NVIDIA. Import the game, set the displayed
option, Quit Redunar, then click Play in native Steam. Inspect the new session,
actual metrics/Replay readiness, save/playback and natural cleanup. On failure,
the wrapper continues the original game command and the app reports unavailable
state. Reverting this source change leaves existing stored profiles readable;
the older wrapper requires app-initiated preparation again.
