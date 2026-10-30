# Steam Play capture task

- Acceptance: the existing `redunar-steam-launch --app-id N -- %command%`
  option prepares the imported game's effective profile when Play is clicked
  in native Steam. It starts the sibling Redunar app in the background when
  needed. Steam's literal original command runs once, including on failure.
- Branch/worktree: `feat/steam-play-capture`, base `1996c2c1`, independent
  `worktrees/steam-play-capture` with checkout-owned build paths.
- Owners: platform wrapper/private request transport; native session supervisor
  and background startup/tray; existing daemon capture/coordinator/replay state.
- Boundaries: one session, exact unique imported Steam identity, same-user peer
  credentials and PID identity, no global injection, no config writes, no
  arbitrary command/path accepted by the app. Flatpak remains unsupported.
- Persistence: retain catalog/profile inheritance and saved launch options.
  End/Quit and natural exit use existing history/cleanup; End retains the game
  launch lock. No game is killed. Background startup does not save tray settings.
- Checks: private socket/argv/startup fixtures, ambiguous and busy rejection,
  process exit/reuse, cleanup and repeated launches, both Cargo workspaces,
  strict Clippy, production UI build/visual fixture, package compatibility gate.
- Deferred: live Steam/game/NVIDIA acceptance, installation and publication.
  No network or live game action is authorized by this task.
- Recovery: bounded request/startup waits fail open to Steam; unavailable tray
  exposes the app window. Existing app ownership prevents a second backend.
