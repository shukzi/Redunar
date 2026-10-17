# Original native scrolling restored — October 17, 2026

Branch `feat/workspace-motion`, base `0b71a05774bcc8557f0b0d0859c5827ee11df5f3`.
The owner requests returning to the scrolling used before the wheel-response
trials. This supersedes the [custom browser-scroll controller](2026-10-17-browser-scroll.md)
and its earlier tuning records; see the final follow-up in the
[task brief](2026-10-17-wheel-refinement-task.md).

## Change and boundaries

Remove `workspace-wheel.mjs`, its app attachment and its superseded controller
unit tests. The main view enables WebKit native smooth scrolling through the
supported typed getter. Keep the startup-crash repair and independent-view
isolation. Shared WebKit fixtures use the same enabled setting. Wheel input is
owned entirely by WebKit, with no UI event interception or scroll animation loop.

Keep the RAM/VRAM row-height stability fix, unchanged-status DOM retention,
startup-update isolation, lazy microphone discovery, History morphs, dialog/menu/
draft-bar motion, retained Steam setup panel and thumbnail caching/cleanup.
No renderer, system preference, service, persisted format or permission changes.

## Acceptance

Production bundle `main-CkLja9_z.js`, SHA-256
`69cd40990ff59170e3a9ba6aca7e958714cd0589e2a0a24950d8e3a8d92d6484`;
CSS remains `main-tPMih8Qp.css`.

Run the production actual-wheel fixture, refresh performance, Overview geometry,
History morph and real-media Replay fixtures sequentially under private Xvfb.
The wheel fixture checks intermediate native positions, forward/reverse and burst
distance, settling, unchanged page geometry/thumbnails, navigation, no UI wheel
listeners, no prevented wheel events and no JavaScript animation callbacks.
Reduced-motion input stays on the engine path rather than imposing the removed
custom controller's timing/cancellation contract. Inspect the current screenshot
`workspace-ui/overview-native-scroll-restored.png` under the reports directory.
The native Tauri probe must check repeated safe configuration, enabled smooth
scrolling and independent-view isolation without starting application services.

Final gates are `tools/validate.sh full` and
`python3 output/tauri-redunar/build-native.py`, run sequentially against fixed
source. Logs use `.redunar-build/reports/native-scroll-restore-*.log`;
`native-scroll-restore-full-console.log` identifies the full validation report
and adjacent source manifests. The local executable is identified by
`native-scroll-restore-runtime.sha256`. Fixture results are recorded below; final gate/build results are in those logs.

All runtime checks use private fixture state, fake commands/hardware or blank
incognito native views. Nothing is installed; the owner's running app is not
launched or restarted. Xvfb checks do not qualify the owner's physical mouse,
desktop compositor or subjective feel; the local executable is for that manual
acceptance.

### Current fixture results

- 10 actual/synthetic native wheel checks pass.
- Refresh performance (10), Overview geometry (13), History morph (51),
  dialog/dropdown/draft-bar motion (18), Library tab/profile retention (13) and
  thumbnail UI (5) pass with the production bundle and enabled native scrolling.
- Real-media Replay checks pass, including eight real decoded-frame samples.
- The native Tauri probe passes repeated production setup, enabled easing and
  independent-view isolation. The current Overview screenshot was inspected.
- Review: the app no longer imports or installs a wheel controller; the passive
  History scroll-cancellation listener and memory-row line-height fix remain.
  Only the view-local native setting changes. There is no backend/persistence
  migration or new scroll owner needing shutdown. Recovery is the same native
  engine path; physical input feel remains the owner's local acceptance.

## Owner acceptance and local integration

On October 17 the owner tested the local executable, preferred the restored
native scrolling and requested local integration of the reviewed changes.
Retain this behavior together with the workspace motion, Steam-panel retention,
History morphs, thumbnail cache/deletion cleanup and memory-row stability fixes.
The earlier refresh-performance change is already in the base commit.

Stage only this task's files and hunks; retain separate release/upload cleanup
and version-metadata work as uncommitted changes. Fast-forward local `main` from
`0b71a05`, with no remote writes, installation or running-app restart.
Re-run the documented full gate on the resulting checkout. Its dated report,
source manifests, exact integrated commit and unrelated-work preservation audit
are under `.redunar-build/reports/local-integration-*`. Validation of the working
checkout includes the separately retained uncommitted edits, explicitly listed
in its source manifest and status; it is not release-candidate qualification.
