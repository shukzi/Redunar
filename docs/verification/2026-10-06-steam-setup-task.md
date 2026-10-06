# Steam setup discovery task

- Problem: users launching from Steam can miss the per-game Launch Options
  value hidden in Library's Launch matching tab.
- Acceptance: every selected Steam game shows setup status, its native-generated
  value and a Copy button above both Library tabs. Overview guides users to
  setup. Direct games have no Steam setup panel. Unavailable/unconfirmed status
  remains honest; switching games or retrying cannot display an older response.
- Branch/worktree: `fix/steam-setup-discovery`, base `84231a6`, independent
  `worktrees/steam-setup-discovery` with checkout-owned targets.
- Owners: production Library/Overview UI and existing native
  `steam_setup_status` command. Native service/platform retain validation and
  generation of the literal wrapper option; no Steam configuration write.
- Persistence/lifecycle: no new saved state or profile changes. Async updates
  replace only the setup content, preserving drafts and focus during polling.
  Detached panels and superseded requests ignore late results.
- Checks: focused async/error UI regressions, full offline validation of both
  Cargo workspaces, production frontend build and WebKit visual inspection at
  normal/compact widths. No real game, installation or publication.
- Recovery: existing Steam wrapper and capture runtime are unchanged. Manual
  selection/copy remains available if clipboard access fails.
