# Compact Steam setup

- Problem: routine setup status, repeated instructions and a permanent retry
  control make the per-game Steam panel hard to scan.
- Acceptance: implement the owner's Compact mockup: Play from Steam, one paste
  instruction, the exact native launch option and Copy, then a short automatic
  startup note. Confirmed setup has a quiet confirmation and Play instruction.
  Real ambiguity, unavailable setup and failed checks retain reasons and retry.
- Branch/worktree: `fix/compact-steam-setup`, base `4c5723c`, reusing
  `worktrees/steam-setup-discovery` and its checkout-owned build outputs.
- Owners: production `steam-setup.mjs` and the last-loaded `steam-setup.css`;
  Library rendering, tab navigation and focus refresh remain the entry points.
- State/lifecycle: keep native setup evidence authoritative, preserve unrelated
  profile drafts, field selection on unchanged evidence, detached-panel and
  overlapping-response guards. No persistence or service lifecycle changes.
- Boundaries: no Steam configuration writes, hardware capability changes,
  installation, publication or agent-controlled game launch.
- Checks: focused setup regression coverage, full offline validation for both
  Cargo workspaces, production WebKit visual fixture and Debian-compatible app
  build. Reopen the local candidate only when the old app is fully closed.
- Recovery: revert this UI-only change. Capture readiness and the live wrapper
  checks continue to decide whether Replay can run.
