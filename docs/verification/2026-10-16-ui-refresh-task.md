# UI refresh performance task

- Problem and acceptance: unchanged capture status must retain its DOM across
  polling; startup update completion must retain the visible page, playback,
  focus and drafts; microphone enumeration must not block startup.
- Branch: `fix/ui-refresh-performance`, based on `d1a46d1`.
- Scope: frontend status updates, startup update presentation and on-demand
  microphone discovery. Keep renderer, playback normalization, media worker
  limits and thumbnail scroll scheduling unchanged.
- Ownership: `ui/app.js`, `runtime-polling.mjs` and microphone selector markup;
  native service commands retain settings and discovery authority.
- Lifecycle: share an outstanding microphone request, preserve saved source
  identity, show pending/failure states and retain explicit retry. Discovery
  completion must not replace unrelated controls or discard unsaved drafts.
- Boundaries: fake native responses and private generated media only. No host
  installation, app restart, real microphone/game access or external requests.
- Checks: focused Node regressions, production frontend build, isolated WebKit
  performance/workspace/replay/refinements fixtures and visual inspection;
  documented full validation before integration.
- Recovery: revert frontend changes; no persisted formats or native behavior
  change. These checks do not establish live scrolling frame times or qualify
  removing the WebKitGTK crash workaround.
