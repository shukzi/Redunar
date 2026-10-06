# Combined v0.1.14 test candidate

- Request: freeze all combined changes on a separate branch for owner testing,
  then promote and publish the unchanged candidate after successful acceptance.
- Branch/worktree: `release/v0.1.14`, base `7899c1e`, reusing the clean
  `worktrees/steam-setup-discovery` checkout and its own build outputs. The local
  base already includes the reviewed NVIDIA, Replay, Steam and compact UI work;
  local main remains at the base while this candidate is qualified and tested.
- Scope: retain all combined committed fixes, synchronize 0.1.14 metadata and
  prepare local release notes. The owner explicitly excluded the shelved Replay
  diagnostic draft. Older dirty NVIDIA worktrees contain retained pre-integration
  drafts; their later accepted equivalents and safety fixes are already in the
  base, so those old files are not overlaid onto the candidate.
- Acceptance: a clean exact commit, matching version/source/runtime manifests,
  full release validation, all unsigned package formats, and a preserved immutable
  candidate. Owner-controlled ARC Raiders acceptance checks Steam cold startup,
  overlay, Replay readiness, saved video/audio, natural game exit, one session
  record, a reachable background app and a subsequent game launch.
- Ownership: existing native host, shared service, graphics producers and wrapper.
  This task changes version/release inputs only; persistence, inherited profiles,
  runtime permissions, recording and cleanup contracts remain unchanged.
- Boundaries: local builds and fake/private fixtures; no installation, agent game
  launch, GitHub upload, signing-key access or publication. NVIDIA hardware remains
  unverified; the earlier intermittent encoder failure must be reported if it recurs.
- Checks: `tools/check-release-inputs.sh v0.1.14`, exact committed candidate
  qualification through `tools/prepare-release.sh v0.1.14`, package/runtime hashes,
  production UI version inspection and private background-lifetime fixture.
- Promotion: retain the tested commit and bytes. A fast-forward to that exact
  commit changes no tested source; substantive changes or differing runtime bytes
  invalidate relevant acceptance and require a new candidate/checks. Publication
  retains required pipeline checks and separate exact-wording owner approval.
- Recovery: preserve candidates and immutable tags. A failed candidate is not
  published; diagnose before preparing a new version. Never delete recordings.
