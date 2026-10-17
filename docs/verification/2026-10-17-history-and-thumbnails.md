# History directions and thumbnail reuse — October 17, 2026

Task: [scope and acceptance](2026-10-17-history-and-thumbnails-task.md).
Branch `feat/workspace-motion`, based on `0b71a05`, with the earlier motion work
retained. This follow-up has not been installed, launched on the owner's desktop,
integrated into main or published.

## Findings and changes

History previously required identical segment lengths/timestamps, and any
isolated observation disabled the whole transition. Real sensors can begin after
frame telemetry and have different gaps. The old production bundle failed the
partial-coverage motion regression (`history-direction-before.log`). Segments now
morph from matching recorded timestamps within one continuous source segment;
unmatched destination segments fade in place. Isolated samples and gaps remain
exact. The existing duration, point budget, inspection, reduced-motion and idle
cleanup rules remain in force. All twelve directed metric pairs are exercised.

Thumbnails were retained in frontend clip objects only. The shared replay store
now owns an optional private JPEG cache. Native cache hits return bytes before
starting FFmpeg. Identity includes device/inode, size and nanosecond modification
time; rename and trash/restore retain the cache. Cache writes serialize with
deletion and recheck the opened identity, rejecting late results. Permanent native
deletion and existing quota removal clear all cached versions for the inode,
including temporary files, before deleting media. A failed cleanup retains the
recording for retry. Successful cleanup is directory-synced before media removal.
No permanent-delete button or automatic clip-deletion policy is added.

Cache limits: 256 completed images, 256 KiB each, plus one atomic temporary write;
directory scans stop at 4,096 entries. Cache directory/file modes are 0700/0600.
Operations pin the cache directory descriptor, reject symlinks/unsafe identities,
and only touch recognized cache filenames. Read/write failure falls back to normal
generation without blocking playback. Basic metadata and thumbnails load
independently so slow inspection cannot hold a cached image off screen.

## Review and verification

Full validation passed for the implementation at source SHA-256
`4118e8c3277b402a023344c0c07c131a0d09dec2ad821fb42f78742e6028c09e`:
`.redunar-build/reports/20261016T221039Z-full.0ppr6Q.log` and its matching
initial/final source manifests. It covered 822 Rust tests passed, 12 deliberately
ignored hardware tests, 112 Node tests, 19 tooling tests, both Cargo workspace
checks/formatting/strict Clippy, and the Vite production build.

All eight production WebKit fixtures passed sequentially against the built
`main-o_1ho9sL.js` bundle: History 51 checks (including all twelve metric directions),
thumbnail 5, performance 10, Library tabs 13, interaction motion 18, workspace 149,
refinements 24, and Replay media readiness, scrolling/focus, rapid selection,
eight actual generated frame samples, keyboard trimming and export boundaries.
Logs: `.redunar-build/reports/history-thumbnails-<fixture>.log`.
Visually inspected `history-motion-partial-temperature.png` and
`thumbnail-cache-ready.png` under `.redunar-build/reports/workspace-ui/`.
Xvfb emitted its expected DRI3 acceleration warning; these are functional and
visual checks, not a live compositor benchmark.

The tested tree includes unrelated release/documentation edits, preserved without
modification. A concurrent removal of the unrelated `tools/xcb-resize-window.c`
after that full run changed the overall source manifest. The implementation and
built UI did not change; a final full run is recorded in
`.redunar-build/reports/history-thumbnails-final-console.log` with its own exact
source manifests. This results-only document update follows the implementation
checks. No failed checks remain.

Ordinary tests use fake hardware, private replay stores, generated media and
isolated WebKitGTK windows under Xvfb. There are no new dependencies, persisted
profile/ledger changes or permissions granted to the UI.

Separate review pass covers production command registration, service/store
ownership, folder/file changes, secondary owners, failure before deletion,
late worker commits, cache privacy/bounds and graph cancellation. Recovery is to
revert these source changes; older versions ignore the optional cache directory.
No independent reviewer was used. Tests do not establish real compositor FPS,
game frame pacing or hardware/vendor support.
