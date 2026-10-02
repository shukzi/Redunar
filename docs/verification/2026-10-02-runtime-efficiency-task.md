# Runtime ownership and replay efficiency

- Problem: startup ownership is inferred before acquisition; replay saves can
  retain full clips in RAM and wait on storage while holding the runtime lock;
  retained audio counts misreport progress; history and UI work need tighter bounds.
- Branch: `fix/runtime-ownership-and-replay-efficiency`, based on `e589fc3`.
- Acceptance: exclusive primary ownership before cleanup, streamed bounded-memory
  MKV/MP4 saves, nonblocking save preparation, truthful audio progress across
  eviction/reset, bounded private compatible history, polling independent of
  updates, and cached/deferred clip metadata with safe file validation.
- Owners: shared service/session ownership and replay/persistence modules; native
  backend/media commands; frontend startup and clip-library coordination.
- Preserve profile inheritance, pending drafts, recording capability gates,
  atomic saved-clip commits, existing history readers, and user recordings.
- Cover contention, slow storage, duplicate saves, epoch reset/shutdown, failed
  output cleanup, corrupted history, cache invalidation, and failed updates.
- Checks: focused fixture tests, full validation of both Cargo workspaces,
  production build, WebKit fixtures and visual inspection, source/diff review.
- No app install, real-game run, publication, or change to GPU capability policy.
- Recovery: reviewable local branch; retain existing persisted formats and
  recordings. Record final results separately from this plan.
