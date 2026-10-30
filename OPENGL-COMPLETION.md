# OpenGL completion contract

Reviewed September 23, 2026. This document defines the work required before
Redunar may describe OpenGL as finished for its current validated target:
owned direct and native-Steam game launches on x86_64 Linux with AMD/RADV.
Every item below is required unless explicitly listed outside scope. A function
address, synthetic copy, or successful launch proves only its individual step.

Owner acceptance on September 23, 2026 confirmed the installed Fedora RPM's
Stardew Valley OpenGL metrics, menu interaction, and Replay cadence after the
six-slot release-handoff fix. Broader lifecycle and save-matrix checks below
remain separate requirements.

## 1. Metrics path remains production quality

- [x] Intercept GLX, EGL, SDL OpenGL, and SDL renderer presentation without a
  startup timing window or dependency on the capture-library filename.
- [x] Preserve real GL/SDL calls through direct and native Steam launches.
- [x] Publish bounded frame telemetry and in-app session metrics.
- [x] Render Grid, Ribbon, and Telemetry layouts plus Compact, FPS only,
  Detailed, and Custom presets.
- [x] Apply live visibility, position, scale, palette, opacity, branding, and
  metric changes. FPS only remains panel-free.
- [x] Recheck metrics while Replay is active so capture cannot alter overlay
  appearance, frame counts, live settings, or teardown.
- [x] Verify multiple contexts/windows, context replacement, resize,
  fullscreen/window transitions, and exit without stale resources.
  Repeated GLX context replacement, a live 640x360 to 1280x720 resize, and two
  simultaneous GLX windows with independent contexts pass the production
  Replay matrix. An actual X11 fullscreen transition on the September 23 host
  changed 640x360→2560x1440→640x360, reset the encoder at both edges, saved a
  clip, and ended without capture drops/rejects or retained probe FDs/threads.
- [x] Preserve honest unavailable states for missing hardware readings.

## 2. Production frame capture

- [x] Keep diagnostic asynchronous PBO readback bounded and nonblocking.
- [x] Require exact `GL_EXT_memory_object` and
  `GL_EXT_memory_object_fd` tokens and every function used by production.
- [x] Give each producer a private reply socket and drain bounded release
  acknowledgements without lock inversion.
- [x] Reuse an export slot only after GL completion and daemon release.
- [x] Select and document one end-to-end external-memory handle contract shared
  by OpenGL and the Vulkan Video importer. Prove allocation, OpenGL import, FD
  transfer, and Vulkan import compatibility on RADV; do not assume opaque FD and
  DMA-BUF are interchangeable.
- [x] Allocate a fixed pool of GPU-shareable packed RGBA buffers. Provide enough
  release-gated slots for encoder depth while bounding memory per resolution.
- [x] Import allocations into the current GL context as external memory and
  back pixel-pack buffers with them. Validate size, offset, stride, alignment,
  and FD ownership at every boundary.
- [x] Read the game framebuffer asynchronously into a free imported PBO, insert
  a fence, and poll with zero timeout. Never call `glFinish`, wait for encoding,
  or block presentation; drop Replay work when all slots are busy.
- [x] Preserve and restore read framebuffer, read buffer when applicable,
  pixel-pack buffer, pack alignment, row length, skipped rows/pixels, and every
  binding touched by memory-object setup.
- [x] Capture at the same semantic point as Vulkan. Inclusion or exclusion of
  metrics, Replay menu, and Moment saved must be intentional and stable.
- [x] Correct OpenGL's lower-left origin on the GPU or in conversion, without a
  full-frame CPU copy, so saved video has Vulkan-equivalent orientation.
- [x] Generate monotonic fixed-cadence 30/60/120 FPS timestamps. Apply the same
  resolution and encoder gate to 120 FPS.
- [x] Support validated source formats and reject unsupported ones honestly.
  Initial production support may remain packed 8-bit RGBA.
- [x] Recreate buffers safely after resize or context replacement. Retire
  in-flight generations until release, cap them, and pause instead of freeing
  memory still owned by GL or the encoder.
- [x] Clean up GL objects, allocator objects, FDs, sockets, and retired resources
  on normal exit and every partial setup failure.

## 3. Daemon transport and encoding

- [x] Publish `ReplayFrameExported` with exactly one transferred FD and bounded
  source, offset, stride, layout, timestamp, and duration metadata. Pixel bytes
  never enter the datagram protocol.
- [x] Accept OpenGL exports only when the production route is capability-ready.
  Reject malformed, duplicate, out-of-order, foreign-path, missing-FD,
  oversized, or stale-generation input.
- [x] Preserve bounded producer selection for helper-heavy Steam games so an
  idle helper cannot permanently win over the game context.
- [x] Import OpenGL allocations into the existing GPU conversion and H.264
  encoder without CPU readback. Add an explicit input mode if orientation or
  handle semantics differ from Vulkan's linear-buffer mode.
- [x] Return `ReplayFrameReleased` after successful encode, rejected export,
  queue displacement, failure, reset, and shutdown.
- [x] Bound the export queue and drop work safely under pressure.
- [x] Feed encoded OpenGL frames into the existing rolling spool, audio, MKV/MP4
  save, inventory, attribution, trim, playback, and export flows.
- [x] Report Buffering, Saving, Unavailable, and Failed from real native state.
  Source metadata alone never implies a populated buffer. The September 23
  production-path probe saw Buffering, synchronously entered Saving, and
  observed a committed save revision; fault tests cover Failed/Unavailable.

## 4. In-game Replay parity

- [x] Render the Replay menu in OpenGL games with Vulkan-equivalent settings,
  status, duration and format choices, shortcut labels, pointer fallback,
  dismissal, and visibility independent of metrics. The owner accepted the
  Stardew menu on September 23. Production GLX/SDL probes now select format
  and duration by pointer, dismiss via Escape, and save through the menu;
  unit tests cover labels, fallback, and layout independent of metrics.
- [x] Route pointer telemetry and actions through the existing bounded control
  path. Context loss and exit release input grabs: the isolated production
  probe exercises toggle, move, button, ping, Escape and post-close release;
  a daemon regression covers loss of the render target while the menu is open.
- [x] Render Moment saved only after native save completion, independently of
  metrics visibility. Never show it on key press or failed save.
- [x] Preserve shortcuts, app Save replay, audio, output-format changes, and
  future-save duration selection without Vulkan-only assumptions.

## 5. Gating and failures

- [x] Resolve effective global/per-game Replay settings and prepare OpenGL
  transfer only when enabled and supported.
- [x] Keep metrics usable if Replay capability, allocation, encoding, audio, or
  saving fails.
- [x] Surface one stable actionable unavailable reason without per-frame spam,
  private paths, or raw diagnostics.
- [x] Handle absent capability, incompatible handles, allocation/fence/FD errors,
  encoder rejection, resize churn, daemon restart, and producer exit without
  crashes, hangs, or unbounded growth. One-shot failure routes, encoder-startup
  and shutdown regressions, held-release pool saturation, and producer-exit
  cleanup cover the resource boundaries. A daemon-loss regression drives
  10,000 producer presents after its socket peer exits and verifies bounded
  telemetry and clean exit. A restarted daemon begins a new capture session;
  the still-running game is not silently reattached to it.
- [x] Keep vendor-specific work isolated and make no cross-vendor claim.

## 6. Performance and resources

- [x] Record fixed limits for contexts, slots, retired generations, bytes,
  queued exports, per-present work, and logs. See `PERFORMANCE.md`.
- [x] Measure disabled, metrics-only, Replay-active, and saturated paths. Busy
  slots cause drops, not presentation waits. On September 23, a held-release
  GLX route filled all six slots, recorded 318 Replay drops, and still
  presented 599 frames with no transport drop/reject. One-minute 1080p
  metrics-only and 30/60 Replay runs plus the uninstrumented disabled baseline
  are recorded in `PERFORMANCE.md`.
- [x] Validate sustained 30 and 60 FPS at representative 1080p gameplay on the
  development AMD host. Validate 120 FPS only under its existing gate.
  One-minute 1080p GLX/SDL production sessions retained 1,821/3,581 encoded
  packets at 30/60 FPS with zero capture drops or rejects; the
  earlier 1080p120 diagnostic remained subject to its display/encoder gate.
- [x] Verify resize and repeated launch/exit do not leak GPU memory, FDs, sockets,
  threads, or session state. Both one-minute production sessions ended with
  FD count 8→5 and thread count 3→2. Three consecutive GLX/SDL/fullscreen
  production matrices passed in isolated state; context replacement exceeded
  the four-pool budget without exhaustion. Mesa reported 15,341 MiB then
  15,338 MiB free across the repeated-run window, a coarse system-wide check
  rather than per-process VRAM attribution.

## 7. Automated verification

- [x] Test extension matching, allocation arithmetic, slot transitions,
  release/fence ordering, sequence reuse, cadence, orientation, retirement, and
  cleanup.
- [x] Test named reply routing, FD ownership, malformed metadata, duplicate
  sequences, rejected exports, queue saturation, producer handoff, and release
  on every exit path.
- [x] Extend desktop GLX and SDL OpenGL direct/native-Steam routes to use
  production exported buffers and require a playable saved clip rather than
  diagnostic checksums. Keep EGL/OpenGL ES in the metrics/readback matrix until
  it has a separately validated production handle contract.
- [x] Keep the existing six-route metrics and diagnostic suites passing with
  zero transport drops or rejects.
- [x] Run strict OpenGL Clippy, affected daemon/platform tests, the Tauri
  production build, release-script checks, and `git diff --check`.
- [x] Add bounded failure injection for allocation, import, fence, transfer,
  encoder startup, saturation, resize, and shutdown.
  One-shot allocation, import, fence, and transfer failures recovered in the
  September 23, 2026 production Replay matrix. Held-release saturation, actual
  resize/fullscreen codec resets, and an encoder-startup-failure unit test also
  pass. A shutdown-failure backend fixture verifies ownership is retained
  when device-idle cleanup cannot be proven.

## 8. Owner-controlled acceptance

- [x] Produce and inspect a synthetic OpenGL Replay clip for dimensions,
  orientation, cadence, duration, keyframes, playback, and audio.
  The production GLX and SDL tests draw red at the top and blue at the bottom;
  decoded pixels must retain that orientation. Automated MKV/MP4 checks also verify
  dimensions, frame count/rate, duration, a keyframe, Opus audio, and full
  decode. The September 23, 2026 review clips are retained under
  `target/opengl-review-2026-09-23-fixed/`. On September 24, 2026 the owner
  visually played all three retained clips (glx-production-1080p30-mkv.mkv,
  sdl-production-1080p60-mp4.mp4, glx-fullscreen.mkv) and inspected both
  decoded keyframe PNGs, and reported correct red-top/blue-bottom orientation,
  undistorted 16:9 picture, the expected smoother 60 FPS versus stepped 30 FPS
  cadence, and the brief opening tone in each clip, with no defects.
- [x] After owner authorization for each launch, validate metrics plus Replay in
  a direct OpenGL game and Stardew Valley through native Steam.
- [x] Save MKV and MP4 through a shortcut and app action; use the in-game menu;
  change future format; hide/show metrics while buffering; verify Moment saved.
  Isolated production probes exercise menu save (MKV), bounded control save
  (fullscreen route), service action (MP4), future format changes, live metrics
  visibility changes, and notice publication only after commit. Owner real-game
  visual acceptance remains open.
- [x] Exercise window/fullscreen changes, resize, exit while buffering, encoder
  unavailability, and relaunch. Confirm honest app state and retained metrics.
- [x] Record exact build, host, Mesa/driver, route, results, and limitations.
  September 24, 2026 owner run record: build redunar-app-0.1.3-9.local.fc44
  (installed payload verified against its RPM), host Fedora 44 x86_64, kernel
  7.2.7-200.fc44, Mesa 26.2.3 (mesa-libGL and mesa-dri-drivers), AMD Radeon RX
  6800 XT on RADV, route native Steam launch with the private OpenGL
  interposer through LD_PRELOAD. The owner confirmed in-game metrics, Instant
  Replay buffering, live music and effects, and a saved MP4 clip
  (redunar-replay-1790264709-515828288-1.mp4, 29.0 s video with 28.9 s Opus
  audio). Limitations of this record: a single session, an MP4-only save, no
  fullscreen, resize, or exit-while-buffering exercises, and no direct
  non-Steam OpenGL game launch in this session.

Owner decision on September 24, 2026: the owner performed limited real-game
tests of the matrix items above during the redunar-app-0.1.3-9.local.fc44
sessions (saves, in-game menu use, metrics visibility changes, and window
transitions in small tests), then reviewed the remaining full-matrix scope and
decided to skip it and consider the OpenGL path working. The boxes above are
therefore closed by those limited tests plus owner decision, not by a complete
executed matrix; per the project's precedence rule the owner's decision closes
this section. Any future real-game report that contradicts it reopens the
affected item.

## 9. Packaging and documentation

- [x] Stage the final OpenGL library and required runtime support in development,
  RPM, DEB, Arch, and portable outputs without global GL files or root runtime.
- [x] Verify installed binaries and sidecars match the current tested build and
  a fresh Redunar process loaded the installed executable. On September 23,
  the newest Fedora RPM passed the installed-payload check. Three isolated
  fresh launches loaded `/usr/bin/redunar-tauri` and restored the expected
  1280×720 logical window at 2× scale. On September 24, 2026 owner game
  launches through native Steam loaded the installed OpenGL sidecar into the
  actual Stardew Valley process (producer hello, frame exports, and Replay
  buffering confirmed in the redunar-app-0.1.3-9.local.fc44 sessions).
- [x] Update `REPLAY.md`, `DESIGN.md`, `ARCHITECTURE.md`, `PERFORMANCE.md`,
  `TESTING.md`, `HARDWARE-SUPPORT.md`, and `ROADMAP.md` to match final behavior
  and evidence. Current Fedora RPM, installed-payload verification, and
  synthetic production routes are recorded in `TESTING.md`; unsupported
  real-game and cross-vendor claims remain excluded.
- [x] Update third-party notices and provenance for any new dependency.
  The OpenGL crate uses existing workspace `nix`, `redunar-capture`, and
  `redunar-core`; its GBM/OpenGL libraries are host runtime interfaces, not
  bundled source or assets. The existing `nix` notice remains applicable.

## Completion rule

OpenGL is finished for this target only when sections 1 through 9 are complete,
production Replay creates a playable owner-accepted clip, the Replay menu and
Moment saved match the Vulkan contract, affected checks pass, installed
artifacts are verified, and documentation claims no more than the evidence.

Status on September 24, 2026: sections 1 through 9 are complete. Sections 1
through 7 and 9 closed by implementation and checks; section 8 closed by the
owner's synthetic-clip acceptance, limited real-game tests, and the owner's
decision to waive the remaining full-matrix scope. The full offline release
gate passed the same day on the tree containing the audio fix. OpenGL is
therefore finished for the current validated target (x86_64 Linux, AMD/RADV,
owned direct and native-Steam launches), with the documented limitations and
the waiver recorded above.

## Outside this target

- Late attachment to an already-running game.
- Flatpak Steam capture bridging.
- NVIDIA, Intel, ARM/aarch64, non-Linux, or universal driver certification.
- Software live-video fallback, desktop/KMS scraping, or cloud capture.
- Game-only audio isolation beyond the existing system-output contract.
