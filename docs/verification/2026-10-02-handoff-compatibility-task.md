# Producer handoff and compatibility qualification

- Continue on `fix/nvidia-beta-readiness`, preserving its existing changes.
- Repair process-local Replay sequence collisions at the native receiver:
  validate ordering per producer, allocate unique service tokens, and retain
  the originating reply socket and wire sequence through delayed completion,
  source replacement, failed acknowledgement, and producer exit.
- Keep the existing capture wire protocol and persisted profile/clip formats.
  Bound producer/token bookkeeping and do not acknowledge unfinished GPU work.
- Prepare the pinned Debian 12 build image, then run the documented offline
  compatibility build and full release/package gate against the final source.
- Add isolated regression coverage for overlapping/lower producer sequences,
  stale duplicates, pending old-producer inputs, release failures/exit, and
  bounded cleanup. Review the production worker and validation entry points.
- Record exact final source/artifact hashes and any qualification failure.
  No installation, publication, or NVIDIA/game run is part of this work.
