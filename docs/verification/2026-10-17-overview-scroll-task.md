# Overview scroll stability

- Problem: ordinary Overview wheel scrolling feels too floaty and slow to stop.
  WebKit's default easing produces trailing animated positions in a private
  input comparison. Separately, empty RAM/VRAM availability paragraphs reduce
  page height by 45 px on monitor loss and shift content on recovery.
- Branch: `feat/workspace-motion`, base `0b71a05`; continue the current UI work
  without disturbing other local changes.
- Acceptance: retain the availability rows' normal line height through missing
  readings and recovery at desktop and stacked widths, while displaying missing
  measurements honestly. Preserve wheel scrolling, thumbnail geometry and polling.
- Owner clarification: normal scrolling feels too floaty/slow to stop. Compare
  native easing on/off with real wheel input and disable that easing only for
  Redunar's main webview. Preserve renderer setup, trackpad/native event handling
  and the separately requested UI/History motion.
- Owner: Overview system-metrics presentation in `ui/workspace.css`; native
  `daemon_status` reads the cached shared monitor snapshot. No runtime, capture,
  persistence, permission, profile, cancellation or cleanup changes. Main-window
  WebKit settings belong to `src-tauri/src/webview_settings.rs`, applied once
  by native setup; the corrected getter uses the already locked WebKit binding
  directly. See the follow-up startup-crash task for native API regression scope.
- Scope: reserve existing text space and disable native animated wheel easing.
  No scroll handlers, extra polling, renderer flags, permanent layer hints,
  persistent settings or layout measurements.
- Checks: regression must fail against the prior production CSS, pass with the
  fix; production build, isolated WebKit visual inspection, UI tests and full
  validation of both Cargo workspaces. Fixture diagnostics cover delayed
  thumbnails, ordinary idle/live polling, replay save refresh and real X11 wheel
  input in a private display.
- Limits: synthetic state transitions do not establish the cause of every owner
  desktop jump or measure compositor performance. No app installation/restart,
  live game, user state access, external service or publication.
- Review: verify effective stylesheet order and intrinsic line-height behavior,
  failed/partial/recovered readings, repeated polling and compact layouts.
