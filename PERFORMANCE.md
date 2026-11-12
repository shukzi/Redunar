# Runtime performance contract

Redunar measures games; its own work must remain bounded and must not claim an
FPS improvement. Budgets below are engineering targets. A pass applies only to
the exact build, hardware, driver, workload, and test duration recorded with it.

## Budgets

While monitoring without active frame capture:

- average monitoring CPU: below 0.5% of one CPU core;
- hardware sample latency: below 2 ms at p95 and 5 ms at p99;
- RSS growth: no more than 1 MiB during ten minutes of idle monitoring;
- no per-sample callback or unbounded snapshot queue.

During active capture, target less than 1% of one CPU core for Redunar's capture,
overlay, audio, and replay workers. Measure game frame pacing, encoder cost,
CPU, GPU, memory, drops, and backpressure separately.

## Collection rules

- Discover sensors once and sample cached paths at 1 Hz.
- Read CPU frequency and package-energy counters into bounded numeric buffers.
- Read only the aggregate CPU row from `/proc/stat`, with a 4 KiB row limit.
- Scan game processes on a bounded slower cadence or on lifecycle events.
- Publish one bounded latest snapshot; do not enqueue one UI callback per sample.
- Keep frame telemetry, media queues, worker counts, file sizes, records, and
  parsed arguments bounded.
- Keep Vulkan, OpenGL, overlay, and Replay presentation paths nonblocking.
- Reuse GPU resources and drop optional telemetry under contention instead of
  stalling the game.
- Keep native supervision, shortcuts, catalog, artwork, and media discovery
  independent from visible webview polling.
- Stop and join owned workers during shutdown.

NVML power sampling uses the driver's recent-sample buffer when available, with
a bounded entry count and freshness limit. Driver-call time remains part of
hardware sample latency and CPU measurements.

The OpenGL metrics surface reuses its pixel allocation across refreshes. Panel
colors are prepared once per fill on transparent pixels; overlapping surfaces
retain the general blending path. Glyphs at their stored raster size use the
same four gamma-adjusted coverage values directly, without interpolation. Other
sizes retain bilinear sampling. Detailed renders more rows than Compact,
and increased overlay scale increases raster work. Synthetic CPU preparation
checks exclude driver, texture-upload, GPU, and game-presentation costs.

The restricted CPU power helper samples only on request, at most once per
second. It caches sensor paths and sends a bounded aggregate value. The normal
hardware sampler polls its socket without waiting. It launches only when direct
energy reads are denied. An idle connection performs no sampling, and closing
the connection ends the helper. Failed startup is retried after 30 seconds;
unreaped helpers prevent further starts.

## UI and scrolling

The webview enables native WebKit smooth scrolling once during setup. The UI
does not intercept wheel input, classify scroll geometry, or run a JavaScript
scroll animation. Unchanged measurements, labels, drafts, and mounted media
retain their DOM nodes. Visibility observers disconnect when content is
replaced or leaves the feature.

Load saved session timelines only when History is open. A history change
outside that page does not load its graphs. The first visit and explicit
refresh read saved history off the window thread. Keep loaded data for return
visits. Stop periodic webview refreshes while the window is hidden and resume
on return. Native monitoring, session supervision, and shortcuts run
independently.

Scrollbar styling uses one path per engine. WebKit uses the existing pseudo
rules without a competing `scrollbar-color` override, so pressed native overlay
thumbs retain their neutral appearance. Scrolling remains engine-owned.

The square workspace shell adds no page-sized clipping or stacking layer.
Selection feedback measures groups before painting, skips unchanged styles,
and uses two-dimensional translation without a persistent layer hint. Its
spring runs only while a selection or its geometry changes.

Motion is short and functional. Dialogs, dropdowns, draft bars, thumbnail
arrival, and selection feedback use bounded opacity/transform transitions;
reduced motion disables them. No recurring animation or whole-document observer
is added for ordinary workspace interaction.

## Completed session measurements

Whole-session frame summaries accumulate the count and integer nanosecond sum
of every accepted frame interval on the capture receiver. The live 240-frame
window and sampled timeline remain independent. Completed averages divide the
accepted frame count by that duration. Low FPS uses the reciprocal mean duration
of the slowest `ceil(frame_count / 100)` and `ceil(frame_count / 1000)` intervals.
No interpolation or percentile estimate is used.

A min-heap retains at most 100,000 slow intervals. Its allocation is bounded to
1 MiB, plus a maximum 800,000-byte vector used only for final sorting. Ordinary
snapshots do not clone or sort this heap. Exact 1% lows cover up to 10 million
frames and exact 0.1% lows up to 100 million. An exhausted tail yields no value;
whole-session count and duration remain available. The worker releases the
accumulator at teardown. This bound is separate from encoded Replay storage.

The journal retains integer totals and compatible optional summary fields.
Older records cannot reconstruct whole-session summaries from their retained
live window. History displays those summaries as unavailable. Transport drops
or rejected messages also make whole-session summaries unavailable. Timeline
observations remain available independently, with the existing compaction limit.
History retains the latest 64 records. These calculations measure received game
presentation timings, not physical display scanout or unobserved frames.

## Replay bounds

Replay uses a bounded producer/export pool, encoder queue, rolling spool, and
local media store. Resolution, format, queue, memory, device identity, encoder
profile, and frame-rate limits are checked before capture. NVIDIA and AMD paths
use their own bounded interop resources; neither path falls back silently to
software video.

The rolling history is capped at 15 minutes. Saved duration can be shorter when
the first retained keyframe occurs after the requested start. History and
session records have fixed size and count limits. The encoded in-memory Replay
repair window is capped at 256 MiB, and the disk spool queue is capped at 32
MiB of payload. Transient spool pressure is repaired from the in-memory window;
storage failures stop disk-history publication instead of publishing a silent
gap while live capture continues in the bounded RAM window. Temporary media
and cache files are cleaned only when Redunar owns them.

## Measurement procedure

Record:

- source commit, package and executable hashes;
- distribution, desktop session, GPU, driver, display mode, game and settings;
- workload, duration, capture mode, resolution, frame rate and audio backend;
- Redunar CPU/RSS, game frame pacing, GPU/VRAM use, encoder latency, drops,
  output duration, decodability, and cleanup result.

For an idle desktop comparison, match the viewport, renderer, saved inventory,
and settling time. Record whether History was opened. Measure the main process
and its WebKit children. Summed RSS counts shared pages once for each process;
PSS divides those pages among the processes that use them. Report idle CPU as a
percentage of one core over the same sample interval.

Use fake hardware and isolated state for ordinary tests. Use
[REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for live-game checks. Synthetic
fixtures qualify code paths and bounds; they do not qualify physical-display
feel, game compatibility, or universal hardware performance.
