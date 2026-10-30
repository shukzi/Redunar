# Native tray and combined candidate — October 9, 2026

Branch `feat/native-tray-release`, base `a6ebea6` (v0.1.14).
[Task and boundaries](2026-10-08-native-tray-task.md).

## Behavior and ownership

The native Tauri host now owns a direct StatusNotifierItem and DBusMenu client
using existing GTK/GIO APIs. One private connection exports the bundled ARGB
icon, tooltip and typed Open/Quit menu while enabled. Disable and shutdown close
that connection. The production dependency tree no longer includes tray-icon
or libappindicator; no desktop detection, extension install, extra service or
new library is added. The locked license inventory check remains current.

Closing hides only after successful registration and confirmation that the
current watcher has a host and lists this exact connection's item. Another
app's registration, acknowledgement without an item, denial or missing host
cannot allow hiding. Watcher ownership generations and query revisions reject
late replies. Owner/item/host/bus loss restores the main window. Native actions
are scheduled on the main thread; the bus worker never waits on GTK during
joining. Background Steam startup retains its visible fallback without changing
the saved preference. Replay shortcut tooltip updates remain native-owned.

The combined candidate also includes the independently checked
[ultrawide/Intel temperature changes](2026-10-08-ultrawide.md) and
[GLX forwarding guard](2026-10-08-x11-glx.md), plus CachyOS installer fixtures.
The earlier AppIndicator package workaround is superseded and not integrated.

## Verification

October 9 local checks on CachyOS x86_64, with isolated state/fake providers:

- `tools/validate.sh full`: passed both Cargo workspaces and strict Clippy,
  formatting, syntax, production frontend build, 576 shared Rust tests,
  145 Tauri/application/example tests, 73 JavaScript and 13 Python fixtures.
  Hardware-dependent Rust tests remain explicitly ignored.
  Report: `.redunar-build/reports/20261008T220655Z-full.hPbW9F.log`.
  Matching initial/final source manifests had SHA-256
  `b15c5983153ae19d49e4c9f88b0552c4e66b60ec9e2434b0d99e000fe4b8f33c`.
- Six private D-Bus fixtures exercise actual exported objects and a fake watcher:
  icon byte order, introspection during registration, menu layout/properties and
  actions; absence/denial/another app; host loss, owner replacement and delayed
  replies; repeated connections, unregister and bus disconnect; malformed
  requests; bounded failed startup. Full validation also runs them through the
  diagnostic examples that include the production module.
- `desktop_controls_probe` under private D-Bus/Xvfb: passed disabled startup,
  repeated enable/disable cleanup and window recovery, hidden native shortcut
  dispatch, empty bindings and absence of the retired desktop Replay window.
- `native_tray_probe --desktop`: temporary registration succeeded on the actual
  DMS/niri StatusNotifier host with neither AppIndicator library available.
  The host's item list matched its pre-probe list after cleanup. This checks
  registration/cleanup, not human visual/menu acceptance across desktops.
- Production WebKitGTK fixture: 97 workspace checks passed; inspected the
  native-derived resolution/frame-rate message screenshot. It fits the capture
  status strip. Fixture-only playback rejection is expected. Xvfb emitted
  acceleration/accessibility/portal warnings, without assertion failures.
- `tools/check-release-inputs.sh v0.1.15` and locked license inventory passed.

The first full attempt passed tests but strict Clippy rejected items below the
tray test module. Moving the test module to the end fixed that ordering. An
earlier private-bus disconnect test exposed the GIO 0.18 name-watch wrapper's
null-connection panic; direct authenticated NameOwnerChanged signals and the
connection-closed callback resolved it. No failing gate or test was disabled.

These results preceded this evidence summary, final release wording edits and
restoration of checked native window-show/focus errors before disabling the tray.
The final probe rerun covers successful native window recovery.
The exact committed candidate must pass `tools/prepare-release.sh v0.1.15`
before signing or publication. Its immutable reports/source manifest and package
hashes are the qualification authority; the full-check counts above are dated
implementation evidence, not a later release pass.

## Review, limits and recovery

A separate review pass traced saved preference changes/rollback, startup,
background reachability, native hotkey notifications, close, Open/Quit,
watcher replacement, cancellation and shutdown through their production callers.
It also checked combined capture source gates, encoder requests and original
GLX provider preservation. No persisted profile/history/clip or wire layout
changes. Reverting the change requires no state migration or recording deletion.

No installation, real game, GPU recording or publication ran for this report.
KDE and GNOME extension interoperability remain protocol expectations, not new
host qualification. GNOME still needs a compatible tray host. The ARC Raiders
KDE/X11 launch report is not confirmed fixed. NVIDIA recording/save/decode and
performance at 5120×1440 remain unverified; the driver's coded-extent and other
capture/encoding capability gates remain active. Missing Intel sensors remain
honestly unavailable.
