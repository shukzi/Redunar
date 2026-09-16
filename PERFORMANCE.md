# Runtime performance contract

Current engineering budgets, reviewed September 13, 2026. Redunar measures games;
its own work must not meaningfully disturb them. Budgets are acceptance targets,
not a promise of performance improvement or a report that every path passes.

## Budgets

For the cached monitor while no active frame capture is running:

- Average monitoring CPU: below 0.5% of one CPU core.
- Hardware sample latency: below 2 ms at p95 and 5 ms at p99.
- RSS growth: no more than 1 MiB during ten minutes of idle monitoring.
- Bounded UI work: never accumulate a callback or queued snapshot per sample.

During active frame capture, Redunar's processing should stay below 1% of one
core; the metrics overlay shares this budget. Measure replay workers, audio,
helper CPU, GPU encode cost, and game frame pacing explicitly. Do not claim the
whole recorder meets this target from a monitor-only or bookkeeping benchmark.
Record process scope, host, driver, build, workload, duration, and method.

## Collection and presentation rules

- Discover sensors once and sample cached hardware paths at 1 Hz. Use counter
  deltas for CPU load; represent missing/reset counters explicitly.
- Conservative game-process scans run no more than every five seconds unless a
  known lifecycle event requires action. Never spawn a process per sample.
- Publish an overwrite-only latest snapshot, keep histories bounded, and batch
  frame telemetry across process boundaries. Do not redraw unchanged values.
- Keep Vulkan telemetry nonblocking. Do not discover hardware, spawn subprocesses,
  wait for UI/service work, or format routine logs in the presentation path.
- Keep rendering geometry, text, and GPU resources bounded. Reuse resources and
  skip telemetry/overlay/replay work under contention instead of stalling a game.
- Use native supervision and shortcut workers independent of visible webview
  polling. Stop and join owned workers promptly on shutdown.
- Keep catalog/artwork/media discovery outside monitor ticks and frame callbacks.
  Bound file sizes, records, parsed arguments, worker counts, and queues.

## Replay bounds

The current capture/encode pipeline has four conversion/encode slots and a fifth
export handoff context to allow the oldest in-flight export to be released.
Do not reduce the producer pool to the encoder depth without checking fence and
acknowledgement progress. Encoding has no host-pixel/software-video fallback.

Encoded spool commands use a four-item nonblocking queue. Existing packet bounds
limit worst-case queued payload; complete segments are bounded to 32 MiB/four
seconds and 512 entries. The spool retains a 15-minute horizon, with bitrate-based
disk limits rather than RAM proportional to the requested duration. Save work
includes the active tail and preserves the store's 512 MiB filesystem reserve.
The authoritative constants live in `replay_spool.rs`, `replay_encoder.rs`, and
`replay_store.rs` under `crates/redunar-daemon/src/`.

Audio uses fixed 20 ms Opus frames and one long-lived capture process, with
retention capped at 45,000 packets/32 MiB. Audio discovery is bounded; audio
failure must not stop video. The intended output fallback is described in [REPLAY.md](REPLAY.md); it does not
change these resource budgets.

Fixed 120 FPS is subject to the implemented display/surface limits and hardware
validation. A capability check or short encode probe is not sustained frame-pacing
proof. Measure drops, queue pressure, disk latency, memory, GPU use, and output
quality through resize/reset and long sessions.

## Desktop media and history

- Clip selection keeps the rail mounted and preserves scroll/focus. One frontend
  preparation is in flight, with only the newest pending selection retained.
  Thumbnail workers remain bounded; completions update individual images.
- Playback preparation runs off the UI thread, with one private cached MP4,
  bounded output, cancellation/cleanup, and a 30-second native deadline.
- The selected player waits for a decoded frame with a 15-second media deadline.
  A failed preparation or decode offers Retry; it must not spin indefinitely.
- Eight filmstrip samples use one temporary decoder and a fixed 1280×90 RGBA
  canvas (450 KiB). Wait while playing/hidden, cancel on selection/navigation,
  and release the decoder after completion. Each media load/seek has a
  2.5-second timeout. Do not regenerate thumbnails per frame.
- History maps retained samples once and uses bounded/logarithmic lookup for
  cursor inspection. Do not rebuild all traces on every pointer movement or
  smooth away measurements to reduce rendering work. See the
  [calculation contract](output/tauri-redunar/HISTORY-CALCULATIONS.md).

## Measurement procedure

Run from the repository root using a release build. For cached-monitor timing:

```sh
cargo run --release --offline -p redunar-daemon --example monitor_profile -- 60
```

That read-only probe measures the monitor, not the whole app. For a production
Tauri process, measure the current PID with `pidstat`/`ps`, include native helper
processes, and take before/after RSS. Record at least a ten-minute idle interval
for the growth budget. UI, Vulkan, recording, and codec checks are separate.

Use matched game runs with the same scene, display mode, graphics settings, and
other overlays. Report duration, frame-time distribution/lows, variance, CPU/GPU
cost, dropped/rejected work, and cleanup. A shorter or noisier run must not be
presented as proof of an improvement. Owner-controlled procedures live in
[REAL-GAME-TESTING.md](REAL-GAME-TESTING.md).

Fixtures for counter resets, missing sensors, latest-value delivery, bounded
history/queues, cancellation, and prompt shutdown belong in ordinary tests.
Real GPU/game probes are explicit diagnostics, not a prerequisite for editing
unrelated UI copy or documentation. See [TESTING.md](TESTING.md).

## Historical evidence

Do not use stale implementation descriptions or a pass on an older build as
current acceptance.
