# Testing guide

Use the repository's validation entry points and keep ordinary tests isolated.
A test result applies to its exact source, build, package, hardware, driver,
desktop session, and state directory. A build is not an installation, and a
synthetic pass is not real-game acceptance.

## Validation entry points

Run from the repository root:

```sh
tools/validate.sh quick
tools/validate.sh full
tools/validate.sh release
```

`quick` runs formatting, syntax, frontend and workspace checks. `full` adds
Rust tests and strict Clippy. `release` adds compatibility builds and package
release gates. The Tauri workspace is validated through these commands rather
than a root-only Cargo check.

Focused checks include:

```sh
node --test output/tauri-redunar/tests/*.test.mjs
cargo test --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets
cargo clippy --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets -- -D warnings
tools/check-tauri-release.sh
```

Use the package-specific checks in `tools/`, `packaging/`, and
`output/tauri-redunar/` when the change affects those inputs.

Opt-in synthetic overlay CPU measurements use isolated file fixtures and native
render preparation without a game or graphics driver:

```sh
cargo test --locked --offline --release -p redunar-platform cached_cpu_sampling_latency --target-dir .redunar-build/checks/root -- --ignored --nocapture
cargo test --locked --offline --release -p redunar-capture-vulkan -p redunar-capture-opengl cpu_latency --target-dir .redunar-build/checks/root -- --ignored --nocapture
```

These measurements do not establish live-game overhead or hardware sampling
latency. Run them separately from ordinary validation to avoid timing contention.

## Test layers

### Unit and service fixtures

Use fake `/proc`, `/sys`, sensor, GPU, audio, process, socket, and package data.
Cover valid and missing readings, malformed input, stale results, duplicate
requests, cancellation, timeouts, device loss, ownership conflicts, restart,
and cleanup failure. Private fixture state must remain outside user recordings
and installed application state.

CPU power checks cover bounded protocol parsing, delayed and partial replies,
stale data, disconnects, retry limits, and helper rate limiting. Run the ignored
`cpu_power_ipc_latency` platform test in release mode for socket overhead. Native
package acceptance must verify helper mode 0755, policy mode 0644, root ownership,
active-session authorization, rejected arguments, real readings, and helper exit
on disconnect. Fresh-install and upgrade payloads must include both files.

### UI and WebKit checks

Run the production UI build and relevant Node/WebKit fixtures for UI changes.
Verify native scroll behavior, focus, keyboard operation, reduced motion, draft
preservation, polling, overlay settings, playback controls, and unavailable
states. Inspect the effective stylesheet order and the actual built resources.
Do not use synthetic values as native measurements.

Generic WebKit fixtures support X11 or a private GTK Broadway display. Broadway
uses a process-local DPI setting when no monitor is attached; desktop settings
stay unchanged. The trusted pointer fixture still requires X11.
The `output/tauri-redunar/tests/layout-webview.py` fixture covers populated rails,
long History sessions, plot boundaries at wide and narrow window sizes, and
dialog overflow using the production stylesheet order.
The `output/tauri-redunar/tests/toasts-webview.py` fixture covers outlined
notification pills, replacement, foreground reading holds, background expiry,
focus restoration, dialog ownership, draft-bar clearance, and reduced motion.
The matching store tests use controlled clocks and delayed callbacks to cover
system sleep, clock changes, stale timers, bounded bursts, and disposal.
The `output/tauri-redunar/tests/interactions-webview.py` fixture covers native
scrollbar presses and drags, saved toggles, refresh feedback, and microphone
control availability. Platform pointer events are delivered only to its isolated
view; microphone test commands use synthetic responses.

### Capture and Replay checks

Run the finite native probes when the change affects capture, metrics, Replay,
encoding, audio, overlay, or cleanup:

```sh
tools/run-tauri-vulkan-session-acceptance.sh
tools/run-tauri-vulkan-replay-acceptance.sh
```

Check Vulkan and supported desktop OpenGL paths separately. Verify device and
driver identity, format and queue limits, external-memory synchronization,
encoder readiness, resize, device loss, backpressure, timestamp order, media
decodability, audio, and shutdown. OpenGL ES/EGL Replay and Flatpak capture
remain outside this acceptance path.

The daemon session fixtures cover failed initial Replay starts, fresh-frame
encoder recovery, status polling, repeated cleanup, failed acknowledgements and
worker panic. Audio startup is injected in these fixtures, so successful fake
video encoding does not open an audio device. The ignored
`paced_fifteen_minute_pipeline_survives_repeated_storage_stalls` test is the
paced synthetic endurance check. It reports retained duration, segment resyncs,
queue pressure and sampled process RSS:

```sh
TMPDIR=/tmp cargo test --locked --offline -p redunar-daemon paced_fifteen_minute_pipeline_survives_repeated_storage_stalls --target-dir .redunar-build/checks/root -- --ignored --nocapture
```

### Package and installed-runtime checks

Build the exact package affected by the change. Compare its executable,
sidecars, helpers, resources, permissions, signatures, and checksums with the
installed payload only after a separately authorized installation. Verify that a
fresh normal-user process loaded those exact files. Preserve recordings and
settings, and test update/downgrade compatibility with isolated fixtures.

### Real-game checks

Use [REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for native Steam runs. Record
the exact build, package, distribution, desktop session, GPU, driver, game,
mode, resolution, frame rate, audio backend, output duration, and cleanup. Check
launch, overlay, metrics, Replay save/playback, trim/export, audio, shortcuts,
window changes, exit, and relaunch when affected.

## Evidence

Keep current evidence concise and objective: scope, exact build or commit,
commands or run conditions, result, and limitations. Do not claim support from
detection alone. Keep dated implementation reports and private test notes out
of the public source tree; release notes should summarize only the release they
belong to.

## Safety boundaries

Do not run the UI or service as root. Do not install packages, restart the app,
launch a real game, change firmware, overclock hardware, or alter permanent
kernel policy as an incidental part of a build or read-only review. Do not
upload diagnostics or telemetry. Use literal process arguments and bounded
allowlists; never assemble shell commands from user input.
