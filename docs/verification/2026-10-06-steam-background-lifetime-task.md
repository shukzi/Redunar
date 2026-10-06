# Steam background app lifetime

- Problem: after an owner-tested ARC Raiders Replay save with video and audio,
  closing the game leaves Steam and Redunar reporting a live game. The game and
  Proton have exited; the Steam reaper still owns the background Redunar app.
- Acceptance: background Redunar stays available for subsequent games without
  keeping the Steam game reaper alive. Natural game exit releases the existing
  native session lock and records history once. An unavailable startup manager
  must preserve the literal original game command and never spawn a persistent
  app under the Steam reaper as a fallback.
- Branch/worktree: `fix/steam-background-lifetime`, base `163e75a`, reusing the
  clean `worktrees/steam-setup-discovery` checkout and its isolated build outputs.
- Owners: platform Steam session bootstrap and its packaged wrapper; existing
  authenticated launch PID, capture lifecycle, native supervisor and cleanup.
- State: no profile, preference, recording, Steam configuration or persisted
  format changes. Existing-owner requests continue to use the private listener.
- Boundaries: same-user transient service startup with the existing Linux user
  service manager, no root, install, global layer, network or game launch.
- Checks: literal service argv/environment and fail-open fixtures; failed and
  timed-out launcher cleanup; full validation for both Cargo workspaces;
  Debian-compatible runtime and relevant packaging checks. A bounded private
  nongraphics process fixture may verify real user-manager process ancestry.
- Recovery: revert bootstrap only. The current running app remains untouched;
  fresh owner-controlled game acceptance is required for the fixed build.
