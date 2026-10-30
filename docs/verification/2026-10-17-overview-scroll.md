# Overview scroll investigation — October 17, 2026

The owner subsequently reproduced a release startup panic in the generic Rust
settings getter. The native acceptance claim below was incomplete: the Python
fixtures used the supported API and never executed that faulty call. The
[startup repair and native regression](2026-10-17-webview-startup.md) supersede
the generic-getter implementation and its native runtime acceptance.

Branch `feat/workspace-motion`, base `0b71a05`; the
[task brief](2026-10-17-overview-scroll-task.md) defines the scope. Preserve the
other local motion, History and thumbnail changes on this branch.

## Finding and change

The built production frontend in private WebKitGTK fixtures retained its scroll
height during delayed thumbnail insertion, ordinary idle/live polling, completed
save refreshes, and native X11 wheel input in Xvfb. Wheel scrolling in either
direction retained the same width and page extent. These observations do not
measure the owner's compositor or prove every desktop jump is absent.
Holding startup catalog loading while scrolling to 300 px also retained that
position through initialization and subsequent polling. Loading recent captures
increased page extent as expected. The owner clarified that the problem occurs
simply opening the app and scrolling; that ordinary desktop behavior remains
unreproduced here. Do not present the monitor-loss repair as a confirmed fix for
the owner's exact symptom.

The owner subsequently described the view as reasonably stable and questioned
the scrolling effect/smoothness itself. Local WebKitGTK 2.54.1 exposes
`enable-smooth-scrolling` with default `true`; Redunar and the locked Wry 0.55.1
previously left this unchanged. The owner identified the problem as too floaty
and slow to stop. A private X11 wheel comparison found each notch generated many
trailing animated positions with that default, versus a single settled position
with native easing disabled. Page width and height remained unchanged in both.

`src-tauri/src/webview_settings.rs` disables that native easing on the main
window's own settings object during setup. The corrected implementation uses
the already locked WebKit binding's supported getter/setter; no new crate,
persistent preference, system setting, renderer
flag or JavaScript wheel handler is added. Failure to dispatch configuration is
nonfatal and reported through the existing operation log. Normal and hidden
Steam startup share this setup. Native input ownership, touchpad event handling,
dialog/dropdown/draft-bar entry and History morphs retain their existing paths.
The shared Python fixture setup mirrors this setting for both workspace and
Replay WebKit views. Actual desktop/touchpad feel still requires owner feedback.

A separate, reproducible layout shift occurs when monitor readings disappear:
the empty RAM/VRAM availability paragraphs lose their line boxes. At 1440×800,
page height fell from 1511 to 1466 px, clamping bottom scrollTop from 739 to
694 px. Recovery restored the height but left the view displaced. The combined
hardware/Replay failure diagnostic also shifted height; isolation identified
hardware loss as the trigger in this fixture, not Replay status loss.

`ui/workspace.css` now reserves one normal line for each memory paragraph,
including empty availability rows. Missing readings remain unavailable; no stale
values are retained. This CSS change adds no event handler, polling or animation.
Existing stylesheet order retains its minimum through compact layouts. The
reproduced layout issue is distinct from the native easing preference above.

## Verification

The regression failed against the previous production bundle with the exact
1511→1466 px shrink above (`overview-scroll-before.log`). Current results are
recorded under `.redunar-build/reports/`; no app installation, app restart, live
game, user data, external services or publication are part of these checks.

- Production bundle: `main-CkLja9_z.js`, `main-tPMih8Qp.css`; changed stylesheet
  SHA-256 `28e3c2dc6da72ff34348b2705fcd5391ca43345d4e3b3829f98e0d3d8fa323eb`.
- `tools/validate.sh full` passed on the native change: both Cargo workspaces'
  formatting/checks/tests/strict Clippy, production frontend, 822 Rust passes
  (12 hardware cases ignored), 112 Node checks and 19 tooling tests. Initial log:
  `20261016T222942Z-full.X0V08D.log` (UTC, October 17 locally).
- Isolated real X11 wheel input: 3 checks passed, including one notch, direction
  reversal, no trailing positions and retained page extent/thumbnails. Log:
  `overview-scroll-wheel.log`. No input was sent to the user's desktop.
- Before the native change, the separate CSS repair passed 13 Overview checks,
  10 existing performance checks, 5 thumbnail checks and 149 workspace checks.
  Logs: `overview-scroll-{after,performance,thumbnail,workspace}.log`.
- With the final native scrolling setting mirrored in both fixture views, all
  ten sequential WebKit fixtures passed: native wheel (3), Overview geometry
  (13), performance (10), thumbnails (5), entry motion (18), History morphs (51),
  Library (13), refinements (24), workspace (149), and Replay media/scroll/focus/
  trimming/export boundaries including eight generated frame samples. Logs:
  `overview-direct-{overview-scroll,performance,thumbnail,motion,history-motion,library-tabs,refinements,workspace,replay}.log`
  plus `overview-scroll-wheel.log`. No app/user recording was opened.
- Visual inspection: `workspace-ui/overview-scroll-stable.png`; existing capture
  thumbnails and compact layout retained. Default Xvfb DRI3 warnings do not
  constitute actual desktop acceleration/performance evidence.
- Review: CSS space matches existing `1.65` paragraph line height; native setup
  changes only the main webview's built-in wheel easing. No ownership/state or
  renderer changes, wheel interception, recurring callbacks or new dependencies.
  `git diff --check` and report links pass.
- Final gate console: `overview-scroll-final-full-console.log`. Source snapshots
  before/after that gate are `overview-scroll-final-initial.source.json` and
  `overview-scroll-final.source.json`; these identify the final local source,
  including the private-fixture adapter and documentation follow-up.

This follow-up is not a merge, installation or package qualification. The direct
wheel response is verified in a private display; owner desktop feel remains a
manual acceptance step. Nothing was installed, restarted or tested in a game.
