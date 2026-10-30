# Steam background lifetime verification — October 6, 2026

Base `163e75a`, branch `fix/steam-background-lifetime`, isolated outputs in
`worktrees/steam-setup-discovery/.redunar-build/`.
Validated and compatible-build source SHA-256:
`af4eeb4b36f0f4d417a2a402791135dd013aa3df2641292f6261040d1d43e410`.
Both source manifests contain identical file hashes. This evidence document and
its index link were added afterward; production code and tests did not change.

## Owner report and observed failure

The owner launched ARC Raiders from Steam with Redunar closed. The background
app started; the screenshot showed Ready to save, 122 seconds buffered,
9,684 received and 9,679 encoded frames, and active audio. The owner then
confirmed excellent saved recording with sound. This is owner-reported
save/playback evidence, not independent inspection of the media or a performance
measurement. The running app hash matched the combined compact build:
`d2a1fd8cdaf92e0687756460c9ef3c24f168f3d2346f6fa2f1c24833d4fe313f`.

After the owner closed the game, Steam still displayed Running and Redunar
retained its session. Read-only process inspection found no ARC Raiders,
Proton or Wine process. The only game-marked process was Steam's reaper, with
the persistent background Redunar process as its child. Redunar's own process
scanner already excludes reaper; the authenticated launch PID, which had exec'd
into reaper, remained live and therefore correctly retained the launch lock.
The new bootstrap's separate process group did not prevent reaper adoption.

## Change and review

The platform bootstrap now starts only the sibling background app as a same-user
transient service through the Linux user service manager. It never uses a scope
or direct persistent-child fallback. Only allowlisted desktop, locale and audio
environment values are forwarded literally; game/capture overrides are removed.
Unrelated environment secrets are not copied into service-client arguments.
Failures preserve the original literal game command. A missing manager requires
opening Redunar first; an existing owner's listener needs no manager startup.
Cancellation/error cleanup reaps only the short client, never the app or a game.

The review traced wrapper exec, authenticated session preparation, process
identity, forwarded supervision, capture completion, history and native cleanup.
Existing ownership gates remain intact. No capture/encoder behavior, hardware
gates, profile inheritance, persistence readers, UI or Steam configuration changed.
No third-party source or new dependency was incorporated; no installation or publication ran.

## Checks

- Full validation of both Cargo workspaces, tests, strict Clippy and production
  frontend build passed: `reports/20261006T155000Z-full.bRlmdu.log`.
- Fixtures cover service argv, literal desktop/audio settings, omitted private
  environment, unavailable/failed/stalled startup, bounded client cleanup,
  existing-owner requests and original game argv. The native supervisor test
  covers natural external-game exit, one history record, released launch lock
  and a still-live separate background owner.
- Offline Debian 12 compatible build passed, maximum required glibc 2.34:
  `reports/steam-background-lifetime-build.log`; all six runtime hashes passed.
- Package staging preserved all six hashes. Vulkan competing-symbol binding
  passed: `reports/steam-background-lifetime-layer-binding.log`.
- A bounded nongraphics fixture ran the compatible wrapper and the staged
  wrapper under a private Linux subreaper. A fake background app started with
  the real user manager as parent, remained alive after the fake game exec
  completed, and left the fake reaper with no children. Space/dollar paths and
  literal environment values survived; game/private environment was absent.
  The private app exited and its transient service was collected. Reports:
  `reports/steam-background-lifetime-fixture.log` and
  `reports/steam-background-lifetime-package-fixture.log`.

Compatible app SHA-256:
`aee22925744fc627491844b651b7c69a55fd4b176fde6b79411880c35f5bf863`.
Compatible/staged wrapper SHA-256:
`f11ec48bd5508fcbc34c88f87a6b38680043b7d01fb69086ad5a9d5e2ad568bb`.

## Remaining acceptance

The current real app was not terminated or replaced in memory. Fully Quit it
once to release the existing stuck reaper, then use a fresh process and repeat
ARC Raiders save/playback and natural exit. Steam must return to Play, Redunar
must end the session once, and its background owner must stay reachable. This
corrected build has no new real-game or NVIDIA hardware acceptance. The earlier
intermittent encoder error remains unexplained. Exact release qualification is
still required before shipping. The separate diagnostic draft remains shelved.

Recovery: revert this bootstrap change; no persisted-data rollback is needed.
