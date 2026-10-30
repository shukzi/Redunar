# Wheel response refinement — October 17, 2026

Historical 110 ms treatment; the owner's further tuning and current evidence are
in [Softer wheel response](2026-10-17-wheel-softening.md).

Branch `feat/workspace-motion`, base `0b71a05`; see the
[task brief](2026-10-17-wheel-refinement-task.md). The owner found direct native
steps less floaty but too abrupt. This supersedes the direct-only wheel policy
in the [earlier scroll investigation](2026-10-17-overview-scroll.md).

## Behavior and ownership

`ui/app.js` attaches `workspace-wheel.mjs` once to the outer `.window-content`.
A coarse vertical notch gets a 110 ms cubic ease-out. Repeated notches retain
distance with one frame chain; reversal discards the old destination and starts
from the visible position. Completion leaves no scheduled callback. Keyboard,
pointer, fine input, external scrolling, navigation, focus/visibility loss and
reduced-motion changes cancel at the current position.

The supported WebKitGTK settings API exposes a smooth-scrolling Boolean, without
a duration/easing parameter in the installed GIR/bindings. Keep native easing
disabled using the [repaired typed getter](2026-10-17-webview-startup.md), so the
two effects cannot stack. A real X11 notch in this WebKit reports `deltaY=99`,
pixel mode and `wheelDeltaY=-120`. Only conservative integral pixel steps with
that legacy notch signal qualify. Line/page modes, fine/fractional pixel input,
horizontal/modifier gestures, editing controls and nested scrolling areas retain
native handling. This heuristic identifies granularity, not physical devices.

No stylesheet, content geometry, renderer flag, native startup behavior, service,
profile inheritance, recording, thumbnail format, persistence or dependency
changes. Owning interface/performance/architecture/testing guides describe the
new input policy. Review covered accumulated distance, reversal, bounds, external
position changes, cancellation order, nested controls and the effective styles.

## Evidence and limitations

Production bundle `main-DiNX-UiX.js`, SHA-256
`c98989d21eecc18581b946fb728203f509abcc6691f8106452307c60dbef8105`.
The CSS bundle remains `main-tPMih8Qp.css`.

- Six deterministic Node regressions cover finite timing, idle work, bursts,
  reversal, boundaries, long frame stalls, cancellation and input classification.
- Sixteen private WebKit checks use actual XTest wheel input for intermediate
  positions, exact notch/burst distance, reversal and direct reduced-motion
  response. They also cover keyboard/pointer/focus/external-scroll cancellation,
  fine/nested/control/modifier bypass, preference changes, navigation, geometry
  and thumbnail retention. Settled input schedules no more frame callbacks.
- Fresh production WebKit geometry (13), refresh performance (10), interaction
  motion (18), History morph (51), Library tabs (13), thumbnail UI (5) and workspace
  (149) checks pass. Replay also passes rail/document scrolling, focus/filter
  retention, media readiness, rapid selection, eight real frame samples, trimming
  and export boundaries. Logs: `.redunar-build/reports/wheel-refinement-*.log`.
- Visually inspected `workspace-ui/overview-wheel-refined.png` under the same
  ignored reports directory. Layout and readable unavailable measurements remain.

Final acceptance commands are `tools/validate.sh full`, the documented isolated
native settings probe and `python3 output/tauri-redunar/build-native.py`.
Their current exit statuses and full output are retained in
`wheel-refinement-full-console.log`, `wheel-refinement-native-probe.log` and
`wheel-refinement-release-build.log` under the same ignored reports directory.
The full console prints the exact validation report path, with adjacent initial
and final source manifests; the wrapper rejects source changes during acceptance.
`wheel-refinement-runtime.sha256` identifies the rebuilt local executable.

The handler is non-passive to replace eligible wheel steps; ancestor geometry is
read only for eligible coarse input, and active frames only update scrollTop.
This adds bounded main-thread work. Private Xvfb checks establish behavior and
idle/DOM regressions, not compositor FPS or every device's feel. Physical
wheel/touchpad and Wayland acceptance remain owner testing. No regular app launch,
installation, restart, real game, system preference change or network action.
Rollback removes the UI attachment/module; no saved-data migration is involved.
