# History direction and thumbnail reuse follow-up

- Branch: `feat/workspace-motion`, base `0b71a05`; preserve prior local motion work.
- Acceptance: all directed History metric changes give smooth feedback with
  partial sensor coverage, retaining gaps, exact final readings, reduced motion,
  interruption cleanup and the existing bounded animation budget.
- Acceptance: reuse validated clip thumbnails across restarts and trash/restore;
  permanent native deletion removes the corresponding cache. Cache failure must
  not prevent playback; a failed deletion cleanup retains the clip for retry.
  No new permanent-delete UI is included.
- Ownership: History presentation module and fixtures; thumbnail generation in
  the native host, private cache and deletion lifecycle in the shared replay store.
- Persistence: optional regenerable private JPEG cache, bounded file size/count;
  validate file identity before reuse or commit, including late worker results.
  Trashing retains cached bytes; permanent deletion removes all versions for that
  inode. No recordings, settings or existing ledgers migrate.
- Checks: focused Node/store regressions, production build and isolated WebKit
  visual checks, existing performance/Library/Replay fixtures, full validation.
- No installation, restart, live app/game, external service or GitHub writes.
- Review: inspect production callers, ownership, invalid/changed files, failed
  cache writes, symlinks, cancellation, rapid switches and exact final geometry.
