# Wheel response refinement

- Problem: the owner finds disabled WebKit easing less floaty but too abrupt.
- Branch: `feat/workspace-motion`, base `0b71a05774bcc8557f0b0d0859c5827ee11df5f3`;
  preserve existing motion, thumbnails, native startup repair and unrelated work.
- Acceptance: soften discrete vertical wheel notches with a finite 110 ms ease;
  stop promptly, reverse from the visible position, retain distance and geometry.
- Ownership: a small UI input module attached once to `.window-content` from
  `app.js`; keep the supported native WebKit getter and native easing disabled.
  No services, persistence, capture, permissions or system preferences change.
- Boundaries: leave fine/high-resolution input, horizontal/modifier gestures,
  nested scroll areas, form controls and reduced motion on the native path.
  Wheel events do not reliably identify devices; use conservative notch signals.
- Cancellation: keyboard, pointer, focus loss, route change, hidden document,
  reduced-motion change and external scrolling stop at the visible position.
  One frame callback chain only during input; no idle timer or observer.
- Checks: deterministic input/cancellation tests, actual X11 wheel events in a
  private WebKit fixture, production build/visual inspection, prior performance,
  geometry/motion/history/replay checks and documented full validation.
- Build locally for owner testing without installing or launching the app. No
  real games, installed-app restart, network, GitHub writes or new dependencies.
- Risk: device type is inferred from event granularity, so desktop hardware feel
  remains owner acceptance. Rollback is removal of the UI attachment/module.

## Owner follow-up: soften the improved response

- The owner prefers the 110 ms treatment but requests still smoother scrolling.
- Tune only duration/curve: 150 ms quadratic ease-out reduces the first-frame
  jump while retaining a short finish, distance, immediate cancellation and
  native bypass paths. No new event handlers, dependencies or animation chains.
- Verify deterministic timing/initial movement, real X11 wheel behavior, refresh
  performance and Overview geometry; production build/visual check, documented
  validation and local release build. No regular app launch/install/restart.

## Owner follow-up: continuous mouse-wheel motion

- Owner confirms a mouse wheel: fast scrolling still feels unsmooth after the
  150 ms tuning. Inspect continuity between notches rather than increase duration.
- Reproduction: a second 99 px notch at 50 ms changes the old modeled speed from
  0.8844 to 1.9003 px/ms across adjacent 1 ms samples (2.15x).
- Acceptance: smooth start from rest, preserve velocity on same-direction input,
  exact distance and 150 ms finite finish; reverse immediately without forward
  drift. Keep one pending frame, monotonic bounds, native bypass and cancellation.
- Own changes in the existing UI controller, tests and owning docs. No native,
  persistence, dependency, capture, system-setting or permission changes.
- Check new tests against the old controller, then production WebKit actual-wheel
  bursts/cancellation, geometry, refresh performance, History/Replay regressions,
  visual inspection, documented full validation and a local release build.
- Private fixture state only; no regular app launch, install, restart or game.

## Owner follow-up: Helium browser reference and frame phase

- Owner wants normal browser scrolling and identifies Helium. Installed package
  metadata identifies Chromium; compare its native behavior with the production
  UI and WebKitGTK, without assuming the engines are identical.
- Investigate real X11 wheel input in temporary Helium profiles/private Xvfb,
  D-Bus and a loopback-only network namespace, retaining browser sandboxing.
  No regular browser or app profile, external requests, installs or restarts.
- The current controller can reset its start time after the next RAF timestamp,
  writing the same visible position for that frame. Earlier synchronous model
  tests miss this frame-phase case. Reproduce before any further timing tuning.
- Acceptance: preserve frame-to-frame progress when events arrive between frame
  timestamps; keep a finite finish, smooth velocity, one callback, exact distance,
  reversal/cancellation and native bypass. No longer duration or renderer change.
- Check new regression against old code, then actual Helium/WebKit frame traces,
  production refresh/geometry/History/Replay regressions, full validation and
  a local release build. Native/compositor feel still requires owner acceptance.

## Owner follow-up: restore the original scrolling

- Owner explicitly requests reverting to the scrolling used before these trials.
- Remove the custom UI wheel controller and its superseded model tests. Restore
  native WebKit smooth scrolling through the safe typed per-view getter.
- Keep memory-row geometry, refresh performance, motion, History and thumbnail
  improvements. No services, persistence, capture or system preference changes.
- Update actual-wheel and native startup probes for engine ownership; production
  build/visual inspection, performance/geometry/History/Replay checks, full
  validation and local release build. No regular app launch/install/restart.
