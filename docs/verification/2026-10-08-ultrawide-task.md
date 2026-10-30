# Ultrawide Replay and CPU temperature task

- Problem: 5120×1440 is rejected by rectangular 3840×2160 limits even though
  its pixel count fits the existing 4K frame budget. Intel package temperature
  is omitted by AMD-only sensor discovery.
- Branch/worktree: `fix/ultrawide-replay`, base `a6ebea6`, independent checkout
  `worktrees/ultrawide-replay` and checkout-owned `.redunar-build/` outputs.
- Acceptance: 5120×1440 passes capture, protocol, DMA-BUF, stream and encoder
  shape checks at 60 FPS and Variable; oversized frames remain rejected before
  allocation. Intel coretemp package readings reach the cached native sampler.
  Resolution errors refer to game resolution rather than window size.
- Ownership: shared capture bounds, Vulkan/OpenGL source selection, daemon
  DMA-BUF/stream validation, Vulkan encoder requests, native status copy and
  Linux sensor discovery. Existing launch/profile inheritance is unchanged.
- Bounds: retain 3840×2160 total pixels and existing per-slot bytes/pool counts;
  permit aspect ratios up to 8192 pixels per axis. Driver coded-extent,
  H.264 level/rate, format, identity, import and cleanup gates remain required.
- Persistence/cleanup: no stored format or wire layout change; old readers
  reject newly admitted oversized axes rather than misreading messages. Do not
  acknowledge unfinished GPU inputs or weaken resize/shutdown ownership.
- Checks: focused private-fixture regressions, full documented validation of
  both Cargo workspaces, production frontend build and visual inspection of
  status copy, relevant local runtime/package checks where artifacts permit.
- No installation, game/hardware test, external contact or publication.
  NVIDIA detection/metrics are user-reported; successful encoding is unverified.
  The separate CachyOS/KDE X11 launch report stays unresolved.
- Recovery: revert this branch; no persisted-data rollback is needed.
