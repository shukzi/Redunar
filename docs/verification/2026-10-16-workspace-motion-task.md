# Workspace interaction motion task

- Acceptance: short entry motion for desktop dialogs, Precision dropdowns and
  Save/Discard bars; immediate interaction, dismissal and reduced-motion behavior.
- Owner follow-up: Game settings/Launch matching must not flash the Steam setup
  placeholder or restart native reads; retain the panel, pending check and drafts
  while changing only the selected tab content.
- Owner follow-up: add the website-style History graph morph on metric switches;
  retain exact final data/inspection and bounded work, with cancellation for
  rapid input, hidden/navigation/reduced-motion states and dense/gapped data.
- Owner correction: apply the prototype's secondary/fill fades and allow dense
  histories to morph using bounded temporary extrema geometry. Keep all original
  observations at completion/inspection. Profile edits/reset/discard/save must
  also retain the mounted Steam panel, including pending checks and save errors.
- Branch: `feat/workspace-motion`, based on `0b71a05`.
- Ownership: production `ui/index.html` stylesheet order, shared desktop CSS and
  the existing draft-action presentation in `ui/app.js`. Native commands, service,
  capture/playback ownership, persistence and cleanup remain unchanged.
- Boundaries: animate only opacity and small transforms after user interaction;
  no recurring animation, dependencies or per-poll restarts. History alone uses
  a bounded frame callback chain for user-requested metric changes.
  Dialogs retain their positioning context for fixed dropdowns and focus.
- Failure/cancellation: close/remove/hide immediately; preserve focus, keyboard
  selection, unsaved global/per-game drafts, failed-save retry and session locks.
- Checks: production build, isolated WebKit normal/reduced-motion coverage and
  visual inspection, existing refresh/workspace/replay regressions, full validation.
- Tests use private fixtures and generated media. No installation, live app
  launch/restart, external requests or real-game access are authorized.
- Recovery: revert the frontend changes; no persisted-data migration. Automated
  checks do not establish compositor frame times or promise zero rendering cost.
