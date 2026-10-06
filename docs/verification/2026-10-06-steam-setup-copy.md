# Steam setup copy verification

October 6, 2026. Branch `fix/steam-setup-copy`, base `07358e4` plus the
[wording follow-up](2026-10-06-steam-setup-copy-task.md). Validated source SHA-256:
`528eda48e32c55b1c9898f230bb120e73e062bbb99cb1cd47182ec1c9ced2c23`.
The later verification report and TESTING link are documentation-only additions.

The native catalog maps `Ambiguous(SteamRunning)` to a distinct `steam-running`
UI state. The panel says **Launch option not yet confirmed** and explains:
“Steam is open, so Redunar can’t confirm recent edits here. If you’ve pasted this
value, click Play in Steam to test the connection.” The native technical reason
and contradictory saved-file instructions no longer appear in that panel.
Other ambiguous reasons retain their explanations, unavailable setup retains its
retry behavior, and `configured` remains false for the running-Steam ambiguity.
No detector, live wrapper verification, launch gate or saved state changed.

Checks passed in the retained `worktrees/steam-setup-discovery` checkout:

- `tools/validate.sh full`, exit 0:
  `.redunar-build/reports/20261006T135341Z-full.0KCqWw.log`. Both Cargo workspaces,
  Rust/Node tests, strict Clippy, formatting and production frontend build passed.
  Native mapping coverage distinguishes SteamRunning from account disagreement.
  Seven focused setup tests retain late-response/retry coverage and add the
  specific wording case plus preservation of other ambiguous reasons.
- Production WebKitGTK fixture, 95 checks passed:
  `.redunar-build/reports/steam-copy-visual.log`. Inspected
  `target/ui-review/steam-play-running.png`; the option remains visible/copyable,
  the new plain next step appears, and no configured success is claimed.

A separate review traced native status serialization, the production renderer,
configured/unconfirmed/unavailable branches, and unchanged profile/launch
ownership. No unresolved issue was found. The running local release app was
left intact for the owner's ARC Raiders test; it still contains the previous
wording. This follow-up needs a later native build/restart to appear there.
No release binary or package was rebuilt, installed or published, and no game
was launched for these fixture checks.
