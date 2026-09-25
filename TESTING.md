# Verification guide

Current Tauri and shared-backend checks, reviewed September 24, 2026. Run commands
from the repository root unless explicitly stated otherwise. Use existing offline
dependencies. Root Cargo commands do **not** include the separate Tauri workspace.

## September 25, 2026 NVIDIA beta implementation check

Build `0.1.4` from `117aef7` plus the uncommitted Beta access and NVIDIA
changes: the root daemon `cargo check --offline`, the separate Tauri
`cargo check --offline`, the frontend `npm run build`, root strict Clippy for
the affected daemon/platform/NVML/Vulkan crates, Tauri strict Clippy,
`cargo fmt --all -- --check`, and `git diff --check` passed. No NVIDIA GPU was
available on this host, so NVML readings, in-game NVIDIA metrics, and NVIDIA
Replay recording remain unverified. The browser's local-file policy blocked
visual inspection of the changed Settings view. No tests or game runs were
performed for this change.

## Choose checks for the change

| Change | Required evidence |
| --- | --- |
| Documentation only | Source/decision consistency, local links, and command paths; no app rebuild required |
| Frontend logic | Relevant Node tests and production build; affected UI visually inspected |
| Native commands/lifecycle | Focused Tauri Rust tests, build, Clippy, and relevant bridge checks |
| Shared backend/protocol | Relevant root crate tests, formatting/Clippy, and affected native caller tests |
| Capture/overlay runtime | Fixtures first, then explicitly scoped native/GPU/game validation where needed |
| Packaging/install behavior | Full Tauri release/staging checks; installation only in an authorized workflow |

Do not repeat an expensive full gate without new changes or a reason. A bug fix
needs meaningful regression coverage of its failure, not tests that mirror the
implementation. Ordinary tests use isolated state, fake hardware, and synthetic
media. Never run destructive hardware tests automatically.

## September 23–24, 2026 takeover revalidation

The full offline Tauri release gate passed on an isolated snapshot of the
current dirty working tree. Its native artifacts stayed within the Debian
12/glibc 2.36 baseline. Tauri tests passed (87 passed, 2 ignored), along with
the desktop-controls fixture (9), Replay webview fixture (20), strict Clippy,
all eight Node test files, license checks, installer checks, staging, desktop
metadata, and package builds. The shared Rust workspace test suite also passed.
The snapshot kept the pre-existing 4.7 GB local Tauri build tree untouched.

On September 24, the production WebKit workspace fixture passed 75 checks and
the Replay fixture passed its scrolling, focus, save-duration, media-readiness,
rapid-selection, decoded-frame, and trim/export checks in isolated Xvfb. I
visually inspected the workspace and Replay fixture screenshots; these remain
synthetic UI checks, not owner acceptance of an installed release.

`tools/test-updater-rpm-flow.sh` passed in the cached Fedora 44 container,
including the skipped-transaction case and upgrade/rollback with retained test
history. `tools/test-updater-versioned-rpm.sh` passed the isolated real-binary
0.1.3→0.1.4→0.1.3 upgrade/rollback and executable-hash checks. Both containers
ran without network access and made no host package changes.

The five-asset local candidate bundle is under
[`local-builds/release-candidate-local-test-signature-2026-09-23`](../local-builds/release-candidate-local-test-signature-2026-09-23/STATUS.txt).
Its Fedora 44 x86_64 RPM is version `0.1.3-1.local.fc44`, SHA-256
`705e962cecf3c62ec31b6cafe11427e89e3ace627c6f8cf12794eb486398d247`.
The manifest and all five release assets verify; its signature uses the
included test-only public key, not the production update key. The candidate RPM
payload matches the installed runtime components. No package was installed in
this revalidation. A fresh installed process passed the three isolated 2× Xvfb
window-size cases. The read-only `rpm -V redunar-app` check showed only
user/group ownership flags (`nobody:nobody` on this host); installed content
hashes and modes matched the RPM payload.

The retained synthetic GLX MKV and SDL OpenGL MP4 clips both fully decoded.
They contain 1920×1080 H.264 and Opus; the MKV has 65 video frames at about
30 FPS over 2.17 seconds, and the MP4 has 125 frames at about 60 FPS over 2.08
seconds. I inspected the GLX frame: red is at the top, blue at the bottom, and
the metrics overlay is visible. Owner playback/visual acceptance remains open.

In the September 23 native-Steam retest, Stardew sound remained absent after
`SDL_DYNAMIC_API` was unset; direct Steam launch remained audible. SDL2 and
libpulse were mapped, but no PipeWire sink-input appeared. On September 24,
source review found that OpenGL interposer SDL symbol probes could leave a
`dlerror()` after successful `dlopen()` calls, causing Stardew's OpenAL loader
to reject `libpulse.so.0`. The interposer now clears its internal loader error
after successful loads. The owner reported live Stardew sound restored with
the resulting local RPM; the installed OpenGL sidecar matches the RPM payload.
Instant Replay audio remains untested.

## September 24-25, 2026 overlay renderer redesign

The in-game metrics overlay and Replay menu were rebuilt to the approved
modern reference (soft rounded panels, quiet letter-spaced labels above bright
values, thin dividers, dark-red selected duration cell with red underline,
red-outlined save button). The shared 18x24 coverage tables were regenerated
with a 16-row mono cap and a 14-row UI cap at the original baselines, the eight
palette roles were converted from the reference's sRGB values into the
renderer's linear-light tables, and the Replay menu keeps fixed Redunar
control colors plus its own near-black panel shader slot. The OpenGL interposer
Grid layout now mirrors the Vulkan row structure, and the desktop overlay
preview palette table matches the renderer roles.

Evidence on build `0.1.4` (commit `117aef7` plus these uncommitted
changes), x86_64 Linux, Mesa `lvp` software ICD for the headless pipeline
check:

- Root workspace `cargo test --offline --workspace`: all suites pass
  (redunar-capture-vulkan 68, redunar-capture-opengl 32, redunar-daemon 214,
  plus the remaining crates); the daemon socket test requires an unsandboxed
  run.
- `cargo clippy --offline --workspace --all-targets` and `cargo fmt --check`
  are clean; the Tauri `npm run check` (Node tests plus production Vite
  build) passes.
- `blend_pipeline_is_accepted_by_headless_vulkan` passes with
  `VK_ICD_FILENAMES=lvp_icd.x86_64.json`, accepting the new nine-slot panel
  shader.
- `REDUNAR_CAPTURE_PROBE_OVERLAY=1` against `/usr/bin/vkcube`: 59 frames,
  drops=0, rejects=0, overlay_status=Some(Active).
- `tools/run-opengl-overlay-acceptance.sh` (six GLX/EGL/SDL routes, isolated
  state): all Active with drops=0 and rejects=0.
- Replay menu control path via `REDUNAR_CAPTURE_PROBE_SAVE_VIA_MENU=1` with
  replay eligibility: MENU TOGGLE/MOVE/BUTTON sequence completes, replay
  copies and exports frames, phase=Completed.
- Visual inspection used the plan-level reference dumps
  (`REDUNAR_REPLAY_MENU_REFERENCE`, `REDUNAR_METRICS_REFERENCE`) and the
  OpenGL canvas dump test (`REDUNAR_OPENGL_CANVAS_REFERENCE`) compared
  against `ai-workspace/private/overlay-reference/replay-menu-reference.png`.
  These are renderer dumps from that build, not owner visual acceptance of a
  real game session.

## September 25, 2026 color-format follow-up

The earlier palette tables held squared reference RGB channels. That build sent
them unchanged to an UNORM swapchain and the OpenGL RGBA8 canvas, which made
the menu controls and dividers too dark in actual game-window captures. The
follow-up keeps the fixed palette roles and encodes them for each Vulkan
attachment format; the OpenGL canvas now encodes them before byte blending.

On build `0.1.4` (commit `117aef7` plus the uncommitted overlay changes), an
offline release build of both injected libraries with `--lib` and the daemon
capture probe completed. In isolated Xvfb, a 640×360 `vkcube` window
using the UNORM swapchain showed the injected menu after a 16-second replay
buffer, including its red selected duration and active red-outlined save
control. The captured image is under
`../artifacts/overlay-review-2026-09-25/reworked/`. This is synthetic visual
inspection; an installed app and a real game remain unverified for this build.
The OpenGL metric panel was visually inspected in a separate isolated
`glxgears` capture using the rebuilt GL library; its Replay menu was not
inspected in that run. Automated test suites from the preceding section were
not rerun for this follow-up.

## Automated commands

Shared Rust workspace:

```sh
cargo fmt --all -- --check
cargo test --offline --workspace
cargo clippy --offline --workspace --all-targets -- -D warnings
```

Use `-p <affected-crate>` for focused iterations. Tauri workspace and frontend:

```sh
cargo fmt --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all -- --check
cargo test --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets
cargo clippy --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --all-targets -- -D warnings
node --test output/tauri-redunar/tests/*.test.mjs
tools/build-linux-release.sh
```

The production release/package gate is:

```sh
tools/check-tauri-release.sh
```

It builds Tauri, runs native/Node/lint checks, validates desktop/AppStream metadata
without network access, checks the staging tree and privilege boundaries, and
builds/checks an RPM when `rpmbuild` is installed. It does not install it or run
the ignored hardware/game acceptance tests. Inspect the script when changing
release behavior; do not substitute root-only Cargo success for this gate.

Installer-only checks do not install packages or contact GitHub:

```sh
tools/test-installer.sh
tools/build-tauri-deb.sh target/install-assets
tools/build-tauri-arch-package.sh target/install-assets
tools/build-tauri-opensuse-rpm.sh target/install-assets
tools/build-tauri-portable.sh target/install-assets
tools/render-public-installer.sh example/redunar /absolute/output/install.sh packaging/release-signing-public.pem
REDUNAR_GITHUB_REPOSITORY=example/redunar ./install.sh --check
```

The installer suite covers Fedora, Ubuntu/Debian, Arch, and openSUSE plans plus
old-glibc, ARM, Alpine, NixOS, and immutable-Fedora refusal paths. It also uses
an isolated local download and fake package manager to verify signed-manifest and
checksum success, corrupted-asset rejection, and invalid-signature rejection
without changing the host. Package checks establish structure, dependency metadata,
and signature consistency, not runtime compatibility on those distributions.
The release gate also requires `VERSION` to match the Tauri and RPM package
versions and appear in the signed checksum manifest. Updater unit tests reject
an absent or changed version checksum and parse installed package versions for
the native restart-versus-retry state. The WebKit Settings fixture covers both
states and automatic checks without automatic installation.
On September 23, 2026, the pending-package refresh unit tests passed and the
74-check WebKit fixture passed after manual checks began requesting a signed
channel refresh.
On September 23, 2026, updater unit tests also passed for a locally generated
signed manifest and its tampered variant, plus failed cache-metadata commit and
successful replacement of a legacy cached package. These fixtures do not use
the production private signing key or prove a public update download.
The native updater also passed an end-to-end local signed-feed test using a
temporary RSA key, `file://` metadata, actual download and checksum verification,
cached-package reuse, a newer signed release, and rejection of tampered metadata
and package bytes while retaining the verified package. This tests the native
download path without a public feed or the production signing key.
Native installer-state tests now also cover first handoff, reopening after an
unconfirmed or cancelled handoff, digest-named and legacy cache paths, restart
only after the package database reports the new version, opener failure, and
tampered cached bytes. They do not replace a real desktop package-installer run.
On September 23, 2026, the window-geometry and History-scrollbar source passed
the full offline Tauri release gate, nine focused app-preference tests, and a
75-check isolated WebKit workspace fixture. The fixture rendered a 20-entry
History list and confirmed that the scroll viewport extends into the column
gap while cards remain within the entry column; its screenshot was inspected.
The local Fedora RPM SHA-256 is
`a90a5fe703a2519d55979d84f619187fbca01dc3b580b1e934f3f4f337502a13`.
The same RPM passed `tools/test-updater-rpm-flow.sh` in an offline Fedora 44
container: a skipped transaction left 0.1.3 installed, a metadata-only 0.1.4
upgrade and rollback completed, and fixture history survived. This does not
exercise the desktop installer GUI or change the host package.
The matching release binary and, after host installation, the installed binary
both passed the isolated 2× Xvfb window-geometry probe: V5 physical sizes
1280×720 and 2560×1440, plus V6 logical size 1280×720, each opened at
2560×1440 physical pixels. The installed probe also confirmed that every fresh
process loaded `/usr/bin/redunar-tauri`; no window appeared on the active
desktop or used its saved preferences. `tools/check-installed-tauri-runtime.sh`
matched the current RPM's executable, Vulkan and OpenGL libraries, Steam
wrapper, shortcut helper, desktop entry, AppStream metadata, icon, and uaccess
rule. Host-namespace `rpm -V redunar-app` returned no mismatches. The installed
executable SHA-256 is
`c3f3160209e3eea5a6d1bdd9c986f58b4acfbdc6ea9e1ce7c5f9bf779d7fcb1e`.
The following installed Replay-listener evidence applies to the preceding RPM.
On September 23, 2026, the full Tauri release gate passed for the OpenGL,
updater, and secondary-instance source, including the local signed-feed test.
That preceding local Fedora RPM SHA-256 was
`d93ea51ce4cd62b4442759ffb8e8d83869ca3f56b2a87e07aa2bc6cd0cdaabf0`.
After its same-version host reinstall, `tools/check-installed-tauri-runtime.sh`
matched every packaged runtime component. A fresh process, PID 598952, matched
the installed executable SHA-256
`2d279a7e02aa7e6153bd1ff09c84a158c91c09e3dbf60023e8bf22b7a8e444e7`
and owned the Replay socket. A second installed process remained open while the
primary stayed running and retained the socket. The complete release gate
passed on this source after the code-level write-access guard for updater
commands. The following package transaction evidence was collected on an
earlier RPM,
`2d58e887687f2723853689b89793abe6430fcb8f883e0278bd0fce8ca649b794`.
After that RPM's same-version host reinstall, `tools/check-installed-tauri-runtime.sh`
matched the executable, both capture libraries, Steam wrapper, shortcut helper,
desktop entry, AppStream metadata, icon, and uaccess rule. A fresh
`/usr/bin/redunar-tauri` primary process loaded the new executable and owned
the Replay socket. A second installed process remained read-only for ten seconds
without a socket conflict or primary crash; the primary still owned the socket
afterward. Earlier apparent first-launch conflicts were secondary launches
while an older `/usr/bin/redunar-tauri (deleted)` process still held the socket
across reinstall. A deliberately launched second process had also triggered a
primary Tauri panic from duplicate `main` webview setup before the GTK identity
fix. The final-source isolated Fedora 44 real-binary upgrade/rollback test
passed 0.1.3→0.1.4→0.1.3 and verified executable hashes and retained local
history. This does not validate an in-game OpenGL run on the new package, a
public signed update, or a real desktop installer cancellation.
On September 23, 2026, the same Fedora 44 x86_64 host then upgraded from the
candidate 0.1.3 RPM (SHA-256
`2d58e887687f2723853689b89793abe6430fcb8f883e0278bd0fce8ca649b794`)
to an isolated real-binary 0.1.4 RPM (SHA-256
`cdfeb38880bec9ce3eeaa54335edce03333a50346a5fe0577db1a06aacc4d3e2`).
The running 0.1.3 process kept its original executable hash until stopped;
a fresh 0.1.4 process matched the newly installed binary. The host then rolled
back to the exact 0.1.3 RPM, removed it, and reinstalled it. RPM removal cleared
the packaged binary while the Redunar user-state directory remained present
with the same 41 files and 286,276,492 total bytes before and after. Final
installed-payload comparison passed, and a fresh 0.1.3 process matched the
installed executable and owned the Replay socket. This validates package
transactions and restart on this host, not the in-app signed download or a
desktop-installer cancellation.
Earlier on September 23, the gate passed for the installer-state source; that
local RPM SHA-256 was
`23d24e901f49f48e38df5fa44788168d015f5bd468082acea2f71d9fa0a88abd`.
Earlier on September 23, the gate also passed for the atomic-cache source;
that local RPM SHA-256 was
`3492e955b82a31249d714016d241baca92fca1ea556ef3b1ec689380b99e0f2a`.
Earlier on September 23, the full release gate also passed for the OpenGL
back-buffer and updater-refresh source; that RPM SHA-256 was
`f7fbd2df7eb9824ef2e75343fafc4a8fe65309325ded192ccc8fc74296848ade`.
On September 23, 2026, the full Tauri release gate and 74 WebKit workspace
checks passed for the updater working-tree build. Its local Fedora RPM SHA-256
is `7f8dbbeff27dfd257d5d86d899379361d528df7254e27b6791e49cd3a16b224f`.
After a same-version reinstall, `tools/check-installed-tauri-runtime.sh`
matched every payload component and a fresh `/usr/bin/redunar-tauri` process
remained active. On September 23, 2026, `tools/test-updater-rpm-flow.sh` also
passed an offline Fedora 44 container check: the package manager installed
0.1.3, left it intact when no upgrade transaction was started, upgraded a
synthetic RPM to 0.1.4, and rolled back to 0.1.3 while retaining local history.
The synthetic RPM changes package metadata only. On September 23, 2026,
`tools/test-updater-versioned-rpm.sh` also passed: it rebuilt the native Tauri
app as 0.1.4 in an isolated source snapshot, packaged it, and checked that an
offline Fedora 44 RPM upgrade and rollback installed the exact executable
payload from each RPM while retaining local history. Both tests require a
locally built Fedora RPM and a cached Fedora 44 Podman image; they pull nothing
and change no host packages. The versioned test additionally requires local
build dependencies and can take several minutes. Neither test validates the
public update signature, desktop installer cancellation, running-app restart,
or a host package-manager transaction with real system dependencies.

## Built interface and media

After building, run the relevant WebKitGTK fixture checks:

```sh
xvfb-run -a python3 output/tauri-redunar/tests/workspace-webview.py
xvfb-run -a python3 output/tauri-redunar/tests/replay-webview.py
```

These use isolated frontend fixtures, not real user sessions. Inspect affected
screens at normal and compact widths and with keyboard input. Check empty,
loading, failed, inherited/custom, unavailable, and long-text states. Review
computed styles from the built page, not a CSS file that may be overridden.

To check native window restoration at 2× scale without opening Redunar on the
active desktop, run the isolated Xvfb probe after a production build:

```sh
dbus-run-session -- xvfb-run -a -s '-screen 0 3200x2000x24' python3 tools/check-tauri-window-geometry.py /absolute/path/to/output/tauri-redunar/src-tauri/target/release/redunar-tauri
```

The probe supplies temporary V5/V6 app-preference files, confirms actual X11
window dimensions, and removes its temporary state after exit.

For the real native playback command/stream/WebKit path:

```sh
cargo build --release --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --example replay_webview_probe
xvfb-run -a output/tauri-redunar/src-tauri/target/release/examples/replay_webview_probe
```

The default probe generates isolated H.264/Opus media and requires a decoded frame
and filmstrip samples. An optional clip argument uses a private copy of that
recording. Do not log private stream tokens or modify source clips. Fixture
success does not establish real-game performance or every distribution's codecs.

## Retained engineering diagnostics

The following examples are intentional engineering tools. Keep them unless their
capability has a documented replacement. They are outside the ordinary automated
gate: select one for a specific question, record the build and host, and interpret
only the property it checks.

Synthetic and isolated diagnostics are safe for routine development:

```sh
cargo run --release --offline -p redunar-daemon --example replay_foundation_self_test
cargo run --release --offline -p redunar-daemon --example replay_profile
cargo run --release --offline -p redunar-daemon --example replay_store_profile
cargo build --release --offline --manifest-path output/tauri-redunar/src-tauri/Cargo.toml --example desktop_controls_probe
dbus-run-session -- xvfb-run -a output/tauri-redunar/src-tauri/target/release/examples/desktop_controls_probe
```

`replay_foundation_self_test` uses temporary state and deliberately reports that
hardware encoding and decoded playback are unverified. `replay_profile` measures
bounded in-memory ring insertion with synthetic packets. `replay_store_profile`
writes and removes one synthetic 16 MiB container under the temporary directory.
`desktop_controls_probe` uses an isolated D-Bus/Xvfb session, fixture preferences,
and a pipe-only shortcut helper; it does not open input devices or change host
settings.

These live probes require an explicit, scoped hardware run:

| Probe | Command and scope |
| --- | --- |
| System-output audio | `cargo run --release --offline -p redunar-capture-audio --example output_audio_live_probe` resolves the actual default output, starts temporary silent playback on that exact sink, captures ten Opus packets from its monitor through the selected PulseAudio/PipeWire backend, and terminates both children. It does not capture microphone input or retain audio. |
| Vulkan capture | Build `redunar-capture-vulkan`, then run `cargo run --release --offline -p redunar-daemon --example capture_probe -- /usr/bin/vkcube`. It launches the absolute target through the private layer and cleans isolated state. `REDUNAR_CAPTURE_PROBE_OVERLAY=1` checks rendered overlay submission; `REDUNAR_CAPTURE_PROBE_REPLAY_CANDIDATE=1` requests source eligibility; `REDUNAR_CAPTURE_PROBE_REQUIRE_REPLAY_CANDIDATE=1` makes missing eligibility fail; `REDUNAR_CAPTURE_PROBE_REPLAY_ENCODE=1` performs a real Vulkan Video encode. WSI, frame count, dimensions, FPS, metrics, preset, corner, opacity, scale, and timeout have bounded `REDUNAR_CAPTURE_PROBE_*` overrides in the example source. |
| OpenGL capture/overlay | Build `redunar-capture-vulkan`, `redunar-capture-opengl`, `redunar-steam-launch`, and the OpenGL examples, then run `REDUNAR_CAPTURE_PROBE_OVERLAY=1 REDUNAR_CAPTURE_PROBE_PRESET=fps-only cargo run --release --offline -p redunar-daemon --example capture_probe -- /absolute/path/to/target/release/examples/glx_probe` and repeat with `egl_probe`, `sdl_gl_probe`, and `sdl_renderer_probe`. Both SDL probes deliberately use glibc's versioned `dlsym` directly, bypassing preload lookup like managed-runtime deep binding. The renderer probe also calls `SDL_SetHint` before `SDL_Init`, verifying that Redunar repairs SDL's dynamic table during its initialization event instead of depending on call order or timing. Add `REDUNAR_CAPTURE_PROBE_STEAM_BRIDGE=1` to exercise the one-shot native Steam wrapper handoff and session-private `LD_PRELOAD` path. Each bounded target requires non-empty frame telemetry and `overlay_status=Some(Active)`. Preset, layout, palette, branding, metrics, corner, opacity, scale, and frame-dump paths have bounded `REDUNAR_CAPTURE_PROBE_*` overrides. FPS only must remain panel-free; Detailed and Custom must omit unavailable hardware instead of fabricating values. Frame dumps support visual opacity inspection, but the probe does not automatically compare blended pixels. These probes do not validate Replay source, Flatpak capture, or broad real-game compatibility. |
| KMS discovery/planning | `cargo run --release --offline -p redunar-capture-kms --example kms_replay_probe` enumerates local DRM outputs and constructs 60 FPS plans. It does not capture or encode frames. |
| KMS DRM read | `cargo run --release --offline -p redunar-capture-kms --example kms_drm_read_probe -- 1 DP-2` opens the selected card/connector read path and reports its active dimensions. Replace the example arguments with the intended output. |
| KMS encoder setup | `cargo run --release --offline -p redunar-capture-vulkan --example kms_encoder_probe` creates the production KMS Vulkan Video encoder at its fixed 2560x1440@60 diagnostic request. |
| KMS live replay | `cargo run --release --offline -p redunar-daemon --example kms_replay_live_probe -- 8` performs local KMS capture and hardware encode for the requested frame count. |

The KMS/DRM examples are retained exploratory diagnostics, not the production
per-game Replay path and not evidence of supported desktop capture. The
`redunar-platform` `prepare_capture_layer` example is a low-level manifest helper
for a supplied session directory and layer library; normal sessions prepare this
through the service and should not need the example directly.

`tools/run-opengl-overlay-acceptance.sh` runs the bounded GLX, EGL, SDL OpenGL,
and SDL renderer matrix across direct and native-Steam-wrapper launch plans. It
checks non-empty telemetry, zero transport drops/rejects, and active overlay
submission for Grid, Ribbon, Telemetry, FPS only, Detailed, and Custom paths.
It opens short local graphics probe windows and is therefore an explicit local
hardware acceptance run, not an ordinary unit-test command or a real-game test.

`tools/run-opengl-replay-foundation.sh` runs a diagnostic-only asynchronous PBO
readback matrix over the same direct and native-Steam-wrapper routes. It requires
bounded source metadata, completed copy proofs, correct byte counts and sampled
checksums, active overlay submission, and zero transport drops/rejects. The
producer uses three slots, polls fences with a zero timeout, drops work when all
slots are busy, and restores framebuffer, pixel-pack buffer, alignment, row
length, and skip state before returning to the game. This test does not export a
DMA-BUF, feed the encoder, create a Replay buffer, or establish OpenGL Replay
support. The September 22, 2026 development-host run completed all six routes
with 179 presentation frames each and 61 or more completed readbacks per route.
The same run also retained zero drops/rejects after adding a private producer
reply socket, bounded release polling, and exact
`GL_EXT_memory_object`/`GL_EXT_memory_object_fd` capability checks. A local
`glxinfo` query confirmed both extensions for core, compatibility, and GLES
profiles on the AMD Radeon RX 6800 XT using Mesa 26.2.2.

`tools/run-opengl-replay-acceptance.sh` is the production Replay matrix. It
uses GLX and SDL OpenGL targets through direct and native-Steam-wrapper launch
plans, covers metrics shown and hidden plus 30 and 60 FPS, and requires a real
H.264 clip from the DMA-BUF/Vulkan Video path. `ffprobe` must report the source
dimensions, requested cadence, decoded frame count, and YUV420 output. The
probe target must actually create the requested dimensions and frame count;
exported cadence must remain within 85% of the active presentation window,
while capture reports zero transport drops and rejects. The matrix opens four
short local graphics windows, one sustained 1080p/60 window, one gated
1080p/120 window, a live 640x360 to 1280x720 resize, and six GLX context
replacements in one session, two simultaneous GLX windows with independent
contexts, and four one-shot OpenGL failure stages. The resize must save only
the final codec epoch; the replacement route exceeds the four-pool cap to
prove destroyed contexts return their pool budget. The second window swaps
every frame but must not inflate game metrics or change the Replay source.
The held-release route deliberately withholds all six producer ownership
tokens and requires Replay-only drops while game presentation continues;
one September 23 run recorded 318 producer Replay drops, 599 presentation
frames, and zero transport drops/rejects.
The OpenGL unit suite also drops the daemon side of a private datagram pair,
then drives 10,000 producer presents. It requires bounded metric batches and
clean producer exit. This models daemon loss, while held-release saturation
models the separate case where exported Replay slots receive no daemon
acknowledgements. Neither route promises hot attachment to a restarted daemon.
This is an explicit local hardware acceptance run. The original 13-route
matrix passed on the AMD/RADV development host on September 23, 2026 with
zero transport drops or rejects. A September 23 rerun including held-release
backpressure passed all 14 routes, with 319 producer-only Replay drops in the
new route. The full Tauri release gate also passed for
that local working-tree build based on `659b519`. A later cleanup-only update
to EGL destruction and partial overlay setup passed the six-route overlay
matrix, focused OpenGL tests and strict Clippy, and a fresh production release
build. Its RPM SHA-256 is
`32e180691cff0712b8358739f964e073a34a75054604636ebb3a655320d072b0`.
That earlier RPM passed the installed-runtime hash check, and a fresh
`/usr/bin/redunar-tauri` process remained active after installation. The later
September 23 production Replay build passed `tools/check-tauri-release.sh`,
the 14-route capture matrix, and three consecutive GLX/SDL/fullscreen
production Replay matrices. A final release-acknowledgement fix retained
daemon tokens across brief capture-lock contention; 30 OpenGL unit tests,
strict Clippy, the production Replay matrix, and the full Tauri release gate
passed afterward. That OpenGL-capture milestone RPM SHA-256 was
`e27369fca655794a202eb5b273b2d39cb9e2301e9541c229e93c2cdb957b3f3c`.
After reinstalling that RPM, `tools/check-installed-tauri-runtime.sh` matched
all installed payload components, including the OpenGL sidecar. A fresh
`/usr/bin/redunar-tauri` process remained active, with `/proc` confirming it
loaded the installed executable. RPM packaging strips the binaries, so compare
installed files to the extracted RPM payload, not to the unstripped build.
That installed OpenGL sidecar SHA-256 was
`b155051d30e966fdbbd8c60ba09a19fda44d8efc8f8f42c56815cb21e8c38eec`.
These probes do not replace owner testing in Stardew Valley or another real
OpenGL game.

`tools/run-opengl-production-replay.sh` additionally starts the daemon-owned
Replay export pump through the normal profile launch plan, waits for native
Buffering and Saving, checks menu pointer control and release, and requires a
committed local clip. It runs GLX 1080p30/MKV with metrics visible and SDL
OpenGL 1080p60/MP4 with metrics hidden.
The GLX case chooses duration and format through the menu and saves from it;
SDL uses the app service action after an in-menu MP4 choice. The fullscreen
route saves through the bounded shortcut control protocol. Each case toggles
metrics visibility while Replay remains buffering and publishes Moment saved
only after native commit. Both output formats contain H.264 video and Opus
audio and decode
without errors. Both GLX and SDL cases check decoded top-red/bottom-blue pixel
orientation as well as frame rate/count, duration, and a keyframe. The
September 23, 2026 isolated AMD/RADV run passed with zero capture
drops/rejects. This verifies the actual session and save worker, unlike the
direct encoder matrix above; synthetic clips still need owner visual approval.
Set `REDUNAR_OPENGL_REVIEW_DIR` to an absolute local directory to retain the
validated MKV/MP4 clips and first-frame PNGs after the probe's isolated state
is cleaned up. A September 23 rerun exposed an incorrect OpenGL enum: the
export path used `0x0400` (`GL_FRONT_LEFT`) for `GL_BACK`, producing black SDL
MP4 video. The corrected `0x0405` back-buffer selection passed 31 focused
OpenGL tests, strict Clippy, the three-route production save test, the
14-route direct/native-Steam Replay matrix, and the full offline Tauri release
gate on working-tree build `659b519` plus the fix. The rebuilt local Fedora RPM
SHA-256 is `17a80b799f992ea4007334c4b00e59505c785f6aa8881eb8676be429f69bdfda`.
Review artifacts are under `target/opengl-review-2026-09-23-fixed/`; their
GLX MKV and SDL MP4 SHA-256 values are respectively
`2752b7cb9c84693f1bac6f2f0ec837f8670e1c77fbee4075e4d05d9d16250a42`
and `b949f3af577371e76fb431bdadd89668761ceee04de4ae5d258e80215aeb7798`.
The rebuilt package requires a separate installed-runtime check before any
claim that the host is running this fix.
One-minute 1080p30 GLX and 1080p60 SDL production runs on the same host
completed September 23 with 1,821 and 3,581 encoded packets, zero capture
drops/rejects, and no residual probe FD/thread growth after session teardown.
The GLX production probe also requests and observes an X11 fullscreen/restore
transition; the recorder must reset for both 2560x1440 and 640x360 epochs and
save a decodable clip without capture drops.

## Regression areas

- **Profiles:** fresh/previous-version state, inheritance, stale revisions, unsaved drafts
  across games, partial failures, unrelated preference saves, and restart.
- **Visibility:** start a supported game with metrics hidden, show/hide repeatedly,
  and preserve runtime availability. A recording-settings lock must not block
  visibility-only changes; custom per-game values override inherited global ones.
- **Library:** bounded import, duplicates, ambiguous identity, missing executable,
  retained artwork after uninstall/cache removal, separate poster/banner roles.
- **Replay:** rapid selection/cancellation, rail scroll, native temporary-file
  exhaustion/errors, decode/seek deadlines, resize, save/export failure, source
  preservation, deletion boundaries, and final empty state.
- **Menu:** dedicated native document, transparent exterior, preview isolation,
  focus/Escape/outside-click, hidden main window, and close without app shutdown.
- **Shortcuts/tray:** empty/menu-only/save-only bindings, duplicates, persisted
  clearing, hidden-window dispatch, no disabled tray icon, live preference changes,
  missing providers/access, and recovery of a hidden window after tray loss.
- **History:** raw extrema/spikes, null gaps, real timestamps, units, axis/trace
  alignment, both CPU/GPU series, nearest-sample selection, previous-version and partial data.
- **Lifecycle:** direct/forwarded launch, duplicate instance, launch lock after End,
  natural exit, one history write, cleanup, unsupported capability, and restart.
- **Recovery:** fixture-only exact restoration, ownership conflicts, partial
  application, and crashes. The Tauri application must keep privileged actions
  outside the webview.

The [audio behavior](REPLAY.md#audio-behavior) needs default-output discovery,
PulseAudio compatibility, direct PipeWire fallback, route-change, stall, and
backend-failover checks. Captured output can include other applications.
Media-tool tests cover executable discovery, provider-neutral encoder checks,
cached success/retried misses, missing codecs, timeouts, and bounded probe output.
The RPM gate checks file/capability requirements without pinning a multimedia
package name or declaring replacement. Remaining release work is in ROADMAP.

## Installed app and owner-controlled runs

```sh
tools/check-installed-tauri-runtime.sh
```

This read-only check compares installed binary/sidecars with the release build.
After installing an update, fully quit Redunar, including its tray process, then
reopen it. A new RPM file does not replace an already-running executable. Record
build/version, host, test scope, and failures; do not hardcode old passing counts
as acceptance criteria.

Use [REAL-GAME-TESTING.md](REAL-GAME-TESTING.md) for the small agreed game matrix
and targeted Vulkan/session/replay runners. Do not run a blanket `--ignored`
test suite: select explicit hardware tests and understand their effects.

Old recipes and dated results are not part of the active Tauri release checklist.
