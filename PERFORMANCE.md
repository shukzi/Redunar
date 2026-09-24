# Runtime performance contract

Current engineering budgets, reviewed September 23, 2026. Redunar measures games;
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

The current capture/encode pipeline has four conversion/encode slots. Vulkan
uses a fifth export handoff context. OpenGL uses six release-gated slots per
context: one handoff for encoder completion and a second for SDL swap dispatch.
OpenGL keeps at most four context pools; actual 4K RGBA allocations therefore
remain below 800 MiB per process, while ordinary 1080p contexts use about
48 MiB each. Do not reduce either producer pool without checking fence,
presentation-hook, and acknowledgement progress. Encoding has no
host-pixel/software-video fallback.

Source-size generations share the four-pool process cap. A resize discards
fence-complete local copies from the old generation and waits for daemon-owned
exports before deleting that generation. Explicit GLX, EGL, and SDL context
teardown removes every pool and renderer/readback bookkeeping entry owned by
the destroyed context, preventing repeated context replacement from consuming
the cap. The producer selects the largest current viewport as its one active
presentation source, so an auxiliary window does not double its frame count.
Diagnostic OpenGL readback tracks at most eight contexts separately from the
four production pools. The daemon queues at most eight transferred exports and
tracks at most eight provisional producer paths. Frame telemetry batches at
most 64 intervals or 250 ms. Presentation takes only try-locks, polls fences
without waiting, and drops Replay work when all six slots or four pools are
busy. Repeated malformed-source logging is limited to one entry per five
seconds and omits raw transport errors that could contain private paths.

Encoded spool commands use a four-item nonblocking queue. Existing packet bounds
limit worst-case queued payload; complete segments are bounded to 32 MiB/four
seconds and 512 entries. The spool retains a 15-minute horizon, with bitrate-based
disk limits rather than RAM proportional to the requested duration. Save work
includes the active tail and preserves the store's 512 MiB filesystem reserve.
The authoritative constants live in `replay_spool.rs`, `replay_encoder.rs`, and
`replay_store.rs` under `crates/redunar-daemon/src/`.

Audio uses fixed 20 ms Opus frames and one long-lived capture process, with
retention capped at 45,000 packets/32 MiB. Audio discovery is bounded; audio
failure must not stop video. The default-output behavior is described in
[REPLAY.md](REPLAY.md); it does not change these resource budgets.

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

September 23, 2026 AMD Radeon RX 6800 XT, Mesa 26.2.3
radeonsi/RADV, Fedora 44 kernel 7.2.7, working tree based on `659b519`:

| Synthetic 1080p route | Duration | Presented FPS | Replay packets | Receiver CPU |
| --- | ---: | ---: | ---: | ---: |
| Uninstrumented GLX, 7,200 frames | 61.25 s | ~117.6 from elapsed time | — | no receiver |
| Metrics-only GLX, 7,199 frames | 58.74 s | 122.80 | — | 0 sampled ticks |
| GLX Replay at 30 FPS, 7,199 frames | 60.97 s | 118.29 | 1,821 | 187 ticks |
| SDL OpenGL Replay at 60 FPS, 7,199 frames | 59.87 s | 120.37 | 3,581 | 207 ticks |

All captured routes had zero transport drops/rejects. The Replay probes ended
with 5 FDs and 2 threads, down from 8/3 before recording. A separate 640x360
held-release run kept all six export slots owned by the receiver: 318 Replay
work items dropped while 599 frames presented, with no transport failures.
The workload is a simple swap-loop, and the routes are not identical games;
these numbers establish bounded behavior, not a game FPS prediction or a
per-core CPU budget pass. Three consecutive GLX/SDL/fullscreen production
matrices also passed with bounded FD/thread teardown. Mesa's system-wide free
VRAM changed from 15,341 MiB to 15,338 MiB across those runs; that coarse
reading cannot attribute GPU memory to one process. GPU utilization and
long-session VRAM still need a separate owner-controlled observation.

Run from the repository root using a release build. For cached-monitor timing:

```sh
cargo run --release --offline -p redunar-daemon --example monitor_profile -- 60
```

That read-only probe measures the monitor, not the whole app. For a production
Tauri process, measure the current PID with `pidstat`/`ps`, include native helper
processes, and take before/after RSS. Record at least a ten-minute idle interval
for the growth budget. UI, Vulkan, recording, and codec checks are separate.

For bounded Replay bookkeeping and temporary-store throughput:

```sh
cargo run --release --offline -p redunar-daemon --example replay_profile
cargo run --release --offline -p redunar-daemon --example replay_store_profile
```

The first pushes ten simulated minutes of fixed-size packets through the Replay
ring. The second writes and removes one synthetic 16 MiB container under the
temporary directory. They can expose regressions in those components, but they
do not measure capture, hardware encode, decoded playback, game frame pacing, or
sustained recording. Record the build, host, storage, and observed values before
comparing runs. See the remaining retained diagnostics in
[TESTING.md](TESTING.md#retained-engineering-diagnostics).

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
