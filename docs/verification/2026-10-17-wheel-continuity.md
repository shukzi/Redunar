# Continuous mouse-wheel motion — October 17, 2026

Historical controller continuity fix. The owner's next feedback and a browser
frame-phase defect missed here are recorded in
[browser scroll frame timing](2026-10-17-browser-scroll.md).

Branch `feat/workspace-motion`, base `0b71a05`. The owner confirms mouse-wheel
input and reports that the [150 ms tuning](2026-10-17-wheel-softening.md) feels
fast but still unsmooth. Scope and constraints are in the follow-up section of
the [wheel task brief](2026-10-17-wheel-refinement-task.md).

## Finding and change

The former controller restarted its ease-out from zero progress on each notch,
causing an abrupt speed change during repeated input. In a deterministic model,
two 99 px notches 50 ms apart change speed across adjacent 1 ms samples from
0.8844 to 1.9003 px/ms (2.15x). This proves a controller discontinuity; it does not
measure the owner's compositor or establish the sole cause of their perception.

`ui/workspace-wheel.mjs` now joins the displayed position and velocity to the new
destination with a cubic Hermite segment ending at zero velocity. Starting from
rest ramps up gently. Repeated same-direction notches carry velocity and internal
subpixel position, retaining the pending frame callback. Reversal discards the
old forward speed and destination. Slope limits keep movement monotonic and
prevent overshoot. Further input toward a pending boundary leaves its existing
deadline intact. Events arriving before a frame retain the visible origin and
the reversal reset. The same model now samples 0.8755 to 0.8912 px/ms (1.018x),
consistent with gradual acceleration instead of the former kick.

The 150 ms deadline, exact notch distance, one active frame chain, cancellation,
native input bypass, reduced motion and DOM ownership remain. Production entry
is the existing one-time attachment from `app.js` to `.window-content`; native
settings, renderer flags, services, profiles, media and persistence are untouched.
No new dependencies, handlers, idle timers, observers or retained layers.

## Verification

Production bundle `main-BWCi5XaP.js`, SHA-256
`c2f8606d1c4abdba8b051e0cecce03072c6623ee9d320ae8494e39bb83840f59`;
CSS remains `main-tPMih8Qp.css`.

Ten deterministic controller/input tests cover continuity, rest/start/stop,
distance, bursts, reversals, events before a frame, monotonic boundaries,
cancellation and idle work. Three new regressions fail against the old controller
(velocity kick, abrupt start, prolonged boundary finish); all pass after the fix.
The model comparison is recorded in `wheel-continuity-model.json`.
Sixteen real/synthetic private WebKit wheel checks pass, including immediate
reversal/cancellation, reduced motion, native bypass paths and no idle callbacks.
Fresh refresh performance (10), Overview geometry (13), History morph (51) and
real-media Replay regressions pass sequentially against the production bundle.

Evidence lives in `.redunar-build/reports/wheel-continuity-*.log`: `before`,
`unit`, `input`, `performance`, `overview-scroll`, `history-motion` and `replay`.
Visual inspection uses `workspace-ui/overview-wheel-refined.png`. The final
`tools/validate.sh full` console is `wheel-continuity-full-console.log`; it prints
the exact validation report and adjacent initial/final source manifests, and
rejects source mutations during verification. The local release build is logged
in `wheel-continuity-release-build.log`, with the executable identified by
`wheel-continuity-runtime.sha256`.

Review covers interpolation slopes, rounding, cancellation order, reversal,
early/coalesced input, single callback ownership, startup and native bypass.
Private WebKit/Xvfb and deterministic models establish controller behavior and
idle/refresh regressions, not physical-display FPS or every device's feel. Owner
testing still decides the perceived smoothness. No installed app launch,
installation, restart, real game, recording, system-setting or external-service
action. Rollback restores the previous controller; there is no data migration.
