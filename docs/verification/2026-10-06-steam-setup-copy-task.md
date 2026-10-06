# Steam setup wording follow-up

- Problem: Steam being open appears as “Needs attention” followed by technical
  saved-file wording, making a normal setup limitation look like a failure.
- Acceptance: this specific native reason has a distinct UI state and plain
  instructions to paste the option and launch from Steam. No restart/save-file
  step is implied. Other ambiguous/unavailable reasons remain visible, and no
  unverified setup becomes configured.
- Branch: `fix/steam-setup-copy`, base `07358e4`, reusing the completed
  `worktrees/steam-setup-discovery` checkout and its isolated build paths.
- Owners: native catalog DTO reason mapping and the production Steam setup
  panel. Platform detection, launch gates, capture, profiles and persistence
  remain unchanged. No Steam configuration write or real game launch.
- Checks: typed mapping tests, focused setup rendering regression, complete
  offline validation, production UI build and WebKit visual inspection.
- Recovery: the existing native status and live wrapper validation remain
  authoritative. A running Steam client never implies configured capture.
