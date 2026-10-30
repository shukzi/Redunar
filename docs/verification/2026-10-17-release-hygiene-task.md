# Review and integrate remaining local release hygiene

- Problem: uncommitted release/upload cleanup remains after the workspace
  integration; the owner requests review and local integration only.
- Branch: `chore/review-release-hygiene`, base `160fa5b`.
- Acceptance: release metadata agrees at 0.1.24; the gate rejects stale/missing/
  malformed Tauri config; documentation reflects native-package verification,
  the current tray and explicit update-helper privileges; remove only the
  already listed unused X11 helper. Finish with reviewed local main and no
  pending task changes.
- Ownership: release input shell gate and temporary Python fixtures; Tauri JSON
  version; release, packaging, dependency-notice and historical verification
  documentation. Runtime behavior, profiles, recordings and scroll policy stay
  unchanged.
- Cleanup: trace the deleted helper through production, fixtures, build/staging,
  diagnostics, persisted-data and license requirements. Retain all actual legal
  notices and active fixture/geometry code. Recover its exact original content
  from `160fa5b:tools/xcb-resize-window.c` if needed.
- Checks: compare docs with native updater/tray/stager and active dependency
  graph; exercise release guard success/failure and its old-gate regression;
  check local documentation links and metadata; stage existing built payloads
  into a private directory and check layout/permissions; run fake installer
  fixtures and `tools/validate.sh full` on the candidate commit; fast-forward
  main to that exact commit and verify unchanged source.
- No remote writes, network requests, installs, restarts, real game runs,
  package-manager changes, dependency downloads or release publication.
- This is local integration, not full release-candidate or installed-runtime
  qualification. Ordinary fake tests do not claim cross-distribution support.
