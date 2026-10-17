# Browser scroll frame timing — October 17, 2026

Superseded by the owner-requested [native scroll restoration](2026-10-17-native-scroll-restore.md).
The custom controller described below has been removed.

Branch `feat/workspace-motion`, base `0b71a05`. The owner identifies Helium as
the desired browser reference after the [controller continuity change](2026-10-17-wheel-continuity.md).
See the Helium/frame-phase follow-up in the [task brief](2026-10-17-wheel-refinement-task.md).

## Comparison and defect

Local package metadata identifies Helium 0.19.2.1 as Chromium-based; its bundled
driver/browser report Chromium 155.0.8059.39. Redunar uses WebKitGTK 2.54.1. The
comparison runs the production bundle with fake bridge data in each engine,
using real XTest wheel input. Browser-native baselines bypass only the custom
wheel handler inside the fixture; WebKit's smooth-scrolling Boolean is also
compared on/off. The physical notch produces 120 px in Helium and 99 px in
WebKit, so these are engine comparisons, not equal-distance performance scores.

The custom controller can freeze a frame after each continuing notch. Input
dispatch can happen after the timestamp supplied to the next RAF callback. The
former retarget sets `start=performance.now()`, making that callback's elapsed
time negative: it clamps to zero and writes the previous position again.
The recorded Helium burst repeats intermediate positions 39, 86 and 192 px at
the three continuing notches. Earlier synchronous 1 ms model tests miss this
browser frame-phase ordering, despite correctly finding the velocity defect.

The fix anchors retargeting to the last displayed sample timestamp. Its duration
includes the already elapsed interval, keeping the finish at the latest input
plus 150 ms. The next frame advances from its own timeline. Position, velocity,
exact distance, one pending callback, cancellation, reversal, monotonic bounds
and native bypass remain. No renderer flags, native settings, dependencies,
services, data formats or system preferences change.

In the recorded Helium after-run, intermediate repeated frames drop from three
to zero, while the four-notch distance remains 480 px. WebKit retains its 396 px
four-notch distance and bounded settling. Native Helium and native WebKit still
have different curves; this correction does not promise identical engine feel
or establish the only cause of the owner's reported roughness.

## Checks and isolation

Production bundle `main-Cabd_ojC.js`, SHA-256
`9e91e675d1093c51715cc2bef9644807fd3804e7f424906258ce6856bd73e00c`;
CSS remains `main-tPMih8Qp.css`.

- The added deterministic frame-phase regression fails with the former
  controller; all eleven controller/input tests pass with the correction.
- Sixteen actual/synthetic private WebKit wheel checks pass for settling, distance, bursts, reversal,
  cancellation, reduced motion, native bypass and no idle callbacks. Reversal
  origin is now captured in the input handler, avoiding a fixture race between
  Python JS evaluation and XTest delivery while animation continues.
- Production refresh performance (10), Overview geometry (13), History morph
  (51) and real media Replay checks pass sequentially, with visual inspection of
  `workspace-ui/overview-wheel-refined.png` under the reports directory.
- Final commands are `tools/validate.sh full` and
  `python3 output/tauri-redunar/build-native.py`. Logs are
  `.redunar-build/reports/browser-scroll-full-console.log` and
  `browser-scroll-release-build.log`. The console identifies the exact validation
  report and adjacent source manifests; the wrapper rejects source mutations.
  `browser-scroll-runtime.sha256` identifies the rebuilt local executable.

Controller/fixture logs use `browser-scroll-*.log`. Browser comparison traces are
`helium-scroll-frames-before.json`, `helium-scroll-frames-after.json`,
`webkit-scroll-comparison.json` and `webkit-scroll-frames-after.json` under the
same ignored reports directory. They record the virtual display's frame/input
ordering, not the owner's hardware compositor FPS or every browser preference.

Helium tests use a new temporary profile, private D-Bus/Xvfb and an isolated
network namespace with only loopback enabled. They run as the ordinary uid,
retain the browser sandbox, download nothing and make no external connections.
No source from Helium/Chromium is incorporated or bundled. The owner's regular
browser/app profiles are not launched; nothing is installed or restarted.
No game or real recording is used. Review covers frame-clock ownership, retained
deadlines, early events, reversal/cancellation and the effective styles. Restoring
the former retarget clock reverts this change without any data migration.
