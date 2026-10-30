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

## UI and scrolling

The webview enables native WebKit smooth scrolling once during setup. The UI
does not intercept wheel input, classify scroll geometry, or run a JavaScript
scroll animation. Unchanged measurements, labels, drafts, and mounted media
retain their DOM nodes. Visibility observers disconnect when content is
replaced or leaves the feature.

Motion is short and functional. Dialogs, dropdowns, draft bars, thumbnail
arrival, and selection feedback use bounded opacity/transform transitions;
reduced motion disables them. No recurring animation or whole-document observer
is added for ordinary workspace interaction.

## Replay bounds

Replay uses a bounded producer/export pool, encoder queue, rolling spool, and
local media store. Resolution, format, queue, memory, device identity, encoder
profile, and frame-rate limits are checked before capture. NVIDIA and AMD paths
use their own bounded interop resources; neither path falls back silently to
software video.

The rolling history is capped at 15 minutes. Saved duration can be shorter when
the first retained keyframe occurs after the requested start. History and
session records have fixed size and count limits. Temporary media and cache
files are cleaned only when Redunar owns them.

## Measurement procedure

Record:

- source commit, package and executable hashes;
- distribution, desktop session, GPU, driver, display mode, game and settings;
- workload, duration, capture mode, resolution, frame rate and audio backend;
- Redunar CPU/RSS, game frame pacing, GPU/VRAM use, encoder latency, drops,
  output duration, decodability, and cleanup result.

Use fake hardware and isolated state for ordinary tests. Use
[REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for live-game checks. Synthetic
fixtures qualify code paths and bounds; they do not qualify physical-display
feel, game compatibility, or universal hardware performance.
