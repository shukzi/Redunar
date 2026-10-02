# Runtime ownership and replay efficiency verification

Verified October 2, 2026 on CachyOS x86_64 with Rust 1.97.0, Node 26.10.0,
WebKitGTK 2.52.6, and FFmpeg 9.0.2. Branch:
`fix/runtime-ownership-and-replay-efficiency`, based on
`e589fc31abebb00396b152922c162a235db73ca9`. Changes remain local.

## Source and behavior

The full validation report is
`.redunar-build/reports/20261002T102820Z-full.QoQDmK.log`. Its
`.source.json` manifest SHA-256 is
`2008c9bc09b51d50a14d6b92f1fc2b5d3cb87d58f92bf1c360f0458d5a787ddb`.
The initial and final manifests match. This evidence document and its TESTING
link were added after that run; implementation files were not changed.

- The service acquires process ownership before startup cleanup, retains the
  kernel lease through service clones, and makes competing instances read-only.
- Replay snapshots are queued without waiting on spool disk work. Files are
  pinned at the ordered snapshot boundary and streamed into MKV/MP4. Status
  reads use published spool counters. Shutdown can cancel pending assembly.
- Audio health follows actual successful ingestion, including a full rolling
  buffer and an epoch reset.
- History retains its v1 format, bounds reads and record counts, uses private
  atomic writes, and correctly decodes escaped backslashes.
- Startup update checking is independent of runtime polling; concurrent UI
  refresh requests share one reader and one pending follow-up.
- Inventory/metadata caches are bounded and invalidated by file/state identity.
  Clip descriptors are still validated; symlink/FIFO opens fail safely. Basic
  metadata and thumbnails precede optional selected-clip packet scans.
- WebKit fixtures share production renderer defaults and frontend palette
  definitions. Their recording-mode assertions now match DESIGN.

See [ARCHITECTURE](../../ARCHITECTURE.md), [PERFORMANCE](../../PERFORMANCE.md),
and [REPLAY](../../REPLAY.md) for the owning behavioral and resource contracts.

## Checks

| Check | Result |
| --- | --- |
| `tools/validate.sh full` | Passed: 569 Rust tests, 12 ignored; 62 Node tests; 12 Python tooling tests; both Cargo checks and strict Clippy; formatting and production frontend build |
| Synthetic streamed media | MKV and MP4 decoded with H.264/Opus, an audio gap, 60 video frames, and a 36-second timeline crossing cluster boundaries |
| Failure/resource regression tests | Exclusive concurrent ownership, history bounds/permissions/escaping, full audio retention, snapshot cancellation under index contention, bounded MP4 fragments, pinned files surviving unlink, failed output cleanup preserving an existing clip, cache invalidation, and delayed/failed updates passed |
| Workspace WebKit fixture | All 75 checks passed |
| Replay WebKit fixture | Passed scroll/focus retention, selection races, actual frame samples, trimming, export, and aspect ratios |
| Visual inspection | Compact dirty Global settings and the replay editor screenshots inspected |
| Native production build | `python3 output/tauri-redunar/build-native.py` passed with matching capture components |
| Local packaging | Host payload staging, DEB and portable construction, archive structure, and installer fixture tests passed |
| Metadata/licenses | Both offline cargo-deny checks, locked license inventory, desktop metadata, and AppStream validation passed |

Additional logs use the `runtime-efficiency-` prefix under
`.redunar-build/reports/`. Local package artifacts are under
`.redunar-build/packages/runtime-efficiency-host/`. Native executable SHA-256:
`e7c7f0ad20623408ed8b4bc9c747e64a49a5d1e8010a98fa4d98995404f60424`.

## Review, compatibility, and limits

A separate source review pass checked lease lifetime, save admission/worker
registration, snapshot ordering, failure cleanup, safe opens, cache invalidation,
and persistence readers. It also removed disk-index contention from status reads
and prevented read-only secondaries from starting the shared diagnostic log.
No second independent reviewer was used.

No persisted format migration, dependency change, version bump, or GPU capability
change was introduced. Existing recordings are retained; failures remove only
owned incomplete output. Reverting the branch preserves the existing file formats.

The package checks deliberately used the host build through
`REDUNAR_RELEASE_ROOT`; these artifacts are not Debian-12/glibc-2.36-qualified
release candidates. The compatibility container/release gate, installed-app
acceptance, and real-game/GPU performance tests were not run. Nothing was
installed or published. Memory figures in PERFORMANCE are implementation bounds,
not measured whole-process RSS or an FPS-gain claim. Cancellation is cooperative
between I/O operations and cannot interrupt a filesystem syscall stalled in
the kernel.
